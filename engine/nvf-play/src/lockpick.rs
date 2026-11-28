//! Lockpicking (docs/spec-notes/lockpicking.md): activating a locked door or container
//! whose key the player does not carry opens the lockpicking menu, when the player's
//! Lockpick skill is high enough and a bobby pin is in the inventory
//! (`fnv_game::lockpick::opening`); else a line says why not. In the menu the mouse
//! turns the bobby pin over a half circle (left 0°, up 90°, right 180°) and the movement
//! keys put torque on the lock with the screwdriver (`fnv_game::lockpick::Picking`): the
//! nearer the pin is to the lock's sweet spot the further the lock turns; in the sweet
//! spot it turns all the way and opens. Torque against a lock that has stopped wears the
//! pin; a worn-out pin breaks and is taken from the inventory, and the next one goes in.
//! Force Lock opens the lock at once or breaks it for good. When the lock opens it is
//! unlocked and the menu closes; then the door or container is activated as if the player
//! had pressed E on it (`crate::activate::ActivateNext`): a door opens or leads through, a
//! container's menu opens [observed: owner, playtest list 5 item 64].
//!
//! The first time the menu opens the help message `HelpLockpicking` comes up over it, as
//! the original's tutorial box (`help`); E closes it [documented: tutorial_menu.xml
//! `_PCButton_E`; observed: the owner's screenshot]. Whether it has been shown is kept in the game state
//! (`fnv_game::lockpick::Locks`), as are the locks broken by forcing them. Over the lock
//! (the menu file's `LPM_DetectArea`) the mouse pointer is hidden ([`PointerOverLock`]).
//! When the game ends or the player leaves, the menu fades out ([`LEAVE_SECONDS`]); a pin
//! pulled out on leaving goes back the way it came in, a broken pin twists and its long
//! piece flies off (`lockpick_view.rs`).
//!
//! Debug switch `NVF_PLAY_DEBUG_LOCKPICK=1` (off unless set; a development aid, not part of
//! a release): the menu file's debug meter, the sweet spot, the ring around it and the
//! pin's place on a bar, the pin's health and the two tuning values.
//!
//! It is a `PlayState::Menu`: the world and the game clock stand still. To scripts it is
//! menu 1014 [documented: GECK wiki GetMenuTargetRef]. What is on the screen besides the
//! lock (`screen`) follows `menus\lockpick_menu.xml`; the lock itself is the game's model,
//! drawn by the engine (nvforever `lockpick_view.rs`) into [`LockView`].
//!
//! Keys [documented: the help message `HelpLockpicking` 0x0000017C and the menu file's
//! `_PCButton_F` / `_PCButton_E`]: the mouse turns the pin (the Left and Right arrows too
//! [design]); Forward, Back, Strafe Left and Strafe Right (W, S, A, D) apply torque while
//! held; F forces the lock; E leaves (Esc and Tab too [design]). The buttons can be
//! clicked.
//!
//! [design, #2436] Dedicated Android arc/precision-slider and held Turn controls use the
//! same rule-gated Turn/step paths. A selected controller uses left-stick X for the pin
//! and its right trigger for torque, with neutral-on-open gating (touch module).
//!
//! A picked lock is a crime when the door or container is owned: a [`LockOpened`]
//! message names the owner and a line is logged ("crime hook"); `crate::crime` judges it
//! (`fnv_game::Game::player_opened_lock`).
//!
//! The console's `ShowLockPickMenu` (`slpm`, docs/design/console.md) asks for a practice
//! lock of a difficulty ([`PracticeLock`]): once the console has closed the menu opens on
//! it with the player's skill and pins, whatever the skill; pins wear and break as on any
//! lock, but opening, forcing or breaking it changes no reference, gives no XP and is no
//! crime [hypothesis: a designer's test of the minigame].
//!
//! Test aid `NVF_PLAY_TEST_LOCKPICK=<seconds>:<step>[,…]`: `open:<reference hex>` as if
//! the player had activated that locked reference, `skill:<n>` makes the player's Lockpick
//! that for locks, `pin:<degrees>` puts the pin there, `spot` in the middle of the sweet
//! spot, `off:<degrees>` that far beyond the sweet spot's edge, `torque:<seconds>` holds
//! torque that long, `force` presses Force Lock (`force:<roll>`: with that roll, 0 to
//! 100), `help` closes the help message, `exit` leaves. At exit, when the menu was asked
//! for: "lockpick summary: …".

pub mod help;
pub mod screen;
pub mod touch;

use crate::bindings::{Bindings, Control, ControlInput};
use crate::pause::{AidTime, Paused};
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::{PlaySet, PlayState};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use bevy::window::PrimaryWindow;
use fnv_game::lockpick::{self as rules, Opening, Phase, Picking, Refusal, Settings, PIN_RANGE};
use fnv_sim::FormId;
use fnv_util::words::{tr, trf};
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

/// The menu code scripts see [documented: GECK wiki GetMenuTargetRef, 1014].
pub const MENU_LOCKPICK: u16 = fnv_game::game::menus::LOCKPICK;

/// Degrees the pin turns for a unit of mouse movement [design: 600 units across the half
/// circle].
pub const MOUSE_DEGREES: f32 = 0.3;
/// Degrees a second the arrow keys turn the pin [design].
pub const KEY_DEGREES: f32 = 90.0;
/// After the lock opened, broke, or the last pin broke: seconds until the menu starts to
/// leave [design: long enough to see the lock turned and read the line].
pub const END_SECONDS: f32 = 1.5;
/// Seconds the menu takes to fade out as it closes; a pin pulled out on leaving goes back
/// in this time [design; observed: the owner's screenshot of an opened lock shows the HUD
/// coming back over the lock still drawn, turned all the way].
pub const LEAVE_SECONDS: f32 = 0.5;
/// The pin's movement sound plays once for this many degrees of travel [design].
pub const MOVE_SOUND_DEGREES: f32 = 8.0;

/// Ask for the lockpicking menu on a locked reference (activation, or the test aid).
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenLockpick {
    pub reference: u32,
}

/// A practice lock asked for by the console (`ShowLockPickMenu`), opened once the player
/// is back in the world.
#[derive(Resource, Default, Debug)]
pub struct PracticeLock(pub Option<rules::Level>);

/// A lock was picked or forced open, or a terminal hacked (`crate::terminal`): the crime
/// hook (`crate::crime` reads it). `owner`: the door's or container's owner, or its
/// cell's (`fnv_game::lockpick::owner`); None from a terminal (the crime rules find it).
#[derive(Message, Clone, Copy, Debug)]
pub struct LockOpened {
    pub reference: u32,
    pub owner: Option<u32>,
    pub how: OpenedBy,
}

/// How a lock was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenedBy {
    Picked,
    Forced,
    /// A terminal's hacking game won.
    Hacked,
}

impl OpenedBy {
    pub fn word(self) -> &'static str {
        match self {
            OpenedBy::Picked => "picked",
            OpenedBy::Forced => "forced",
            OpenedBy::Hacked => "hacked",
        }
    }
}

/// The lock's picture, drawn by the engine (nvforever `lockpick_view.rs`) while the menu
/// is open; `screen` shows it behind the menu's text.
#[derive(Resource, Clone, Debug)]
pub struct LockView {
    pub image: Handle<Image>,
}

/// The open lockpicking menu, if any.
#[derive(Resource, Default)]
pub struct LockpickScreen(pub Option<Session>);

/// The bobby pin model's sequence being played (`bobbypin01.nif`, lockpicking.md §2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinClip {
    /// `Forward`: a new pin goes in.
    In,
    /// `Backward` then `Left`: the pin twists and its long piece (`Pin2`) flies off; the
    /// short piece stays in the keyhole until the next pin [hypothesis: by the sequences'
    /// keys, the 0.97-s twist ends where the 0.47-s flight starts; observed: a short piece
    /// in the keyhole after a break on the owner's screenshot].
    Break,
    /// `Forward` played backwards: the pin is pulled out as the player leaves [design].
    Out,
}

/// The help message over the menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Help {
    pub title: String,
    pub text: String,
    /// The box's own words, from the game strings table (`help::BoxWords`).
    pub words: help::BoxWords,
}

impl Help {
    /// A help message: its title, its text with the keys bound now written in
    /// ([`help::expand_keys`]) and the box's words read from `game`'s strings.
    pub fn new(game: &fnv_game::Game<'_>, title: String, text: &str, bindings: &Bindings) -> Help {
        Help { title, text: help::expand_keys(text, bindings), words: help::BoxWords::read(game) }
    }
}

/// The menu file's own words (`menus\lockpick_menu.xml`), read by name from the game
/// strings table when the menu opens (`fnv_game::ui_loading::game_string`: the load
/// order's string settings over NVForever's labels; menu-xml.md §2.4) [design].
#[derive(Clone, Debug, PartialEq)]
pub struct Words {
    /// The information rows' titles: `sLockpickSkillText`, `sPicksRemainingText`,
    /// `sLockLevelText`.
    pub skill: String,
    pub picks: String,
    pub level: String,
    /// The Exit button (`sExit`).
    pub exit: String,
    /// The PC key hints after Force Lock (`sPCMenuHintF`) and Exit (`sPCMenuHintE`).
    pub force_hint: String,
    pub exit_hint: String,
}

impl Words {
    pub fn read(game: &fnv_game::Game<'_>) -> Words {
        let word = |name| fnv_game::ui_loading::game_string(game, name);
        Words {
            skill: word("sLockpickSkillText"),
            picks: word("sPicksRemainingText"),
            level: word("sLockLevelText"),
            exit: word("sExit"),
            force_hint: word("sPCMenuHintF"),
            exit_hint: word("sPCMenuHintE"),
        }
    }
}

pub struct Session {
    /// The locked reference; none (0) for a practice lock.
    pub reference: FormId,
    /// A practice lock (`ShowLockPickMenu`): nothing in the world is opened or broken.
    pub practice: bool,
    pub name: String,
    pub game: Picking,
    /// The status line (a pin broke, the lock opened …) [documented: the menu file's
    /// `LPM_StatusText`, "message on pick break / success"; the words are NVForever's].
    pub status: Option<String>,
    /// Aid seconds when the menu opened (the lock's model comes in from then).
    pub opened: f32,
    /// The pin's sequence and the aid seconds it started.
    pub pin_clip: PinClip,
    pub pin_since: f32,
    /// Aid seconds when the game ended (opened, broken, out of pins).
    pub ended: Option<f32>,
    /// The help message, while it is up (the first time the menu opens).
    pub help: Option<Help>,
    /// The menu file's words.
    pub words: Words,
    /// The menu is leaving: since when (aid seconds) and why.
    pub leaving: Option<(f32, &'static str)>,
    /// Degrees the pin moved since its last movement sound.
    moved: f32,
    /// The test aid holds torque until then (aid seconds).
    aid_torque_until: Option<f32>,
}

impl Session {
    pub fn force_label(&self) -> String {
        // [observed: "Force Lock [15%]", the owner's screenshots as noted 2026-10-11]
        trf("Force Lock [{chance}%]", &[("chance", &format!("{:.0}", self.game.force_chance()))])
    }

    /// How much of the menu is drawn: 1, down to 0 as it leaves.
    pub fn fade(&self, now: f32) -> f32 {
        self.leaving.map_or(1.0, |(since, _)| (1.0 - (now - since) / LEAVE_SECONDS).clamp(0.0, 1.0))
    }

    fn begin_leaving(&mut self, now: f32, why: &'static str) {
        if self.leaving.is_none() {
            info!("lockpick: leaving ({why})");
            self.leaving = Some((now, why));
            self.help = None;
        }
    }

    fn pin(&mut self, clip: PinClip, now: f32) {
        self.pin_clip = clip;
        self.pin_since = now;
    }
}

/// Whether the mouse pointer is over the lock, where the menu hides it [documented: the
/// menu file's `LPM_DetectArea`, "Cursor is invisible in this area"]; read by
/// `player::grab_cursor`.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct PointerOverLock(pub bool);

/// Debug switch `NVF_PLAY_DEBUG_LOCKPICK`: the menu's debug meter (`screen`).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct LockpickDebug(pub bool);

/// A command for the open menu.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum LockpickInput {
    /// Turn the pin by degrees (the mouse, the arrows).
    Turn(f32),
    /// Put the pin at an angle (test aid).
    Pin(f32),
    /// Test aid: the pin in the middle of the sweet spot, or this many degrees beyond
    /// its edge.
    Spot,
    Off(f32),
    /// Test aid: hold torque this many seconds.
    Torque(f32),
    /// Force Lock, with the roll given or a random one.
    Force(Option<f32>),
    /// Close the help message.
    CloseHelp,
    Exit,
}

#[derive(Resource, Default, Debug)]
struct LockpickStats {
    asked: u32,
    opened: u32,
    refused: u32,
    picked: u32,
    forced: u32,
    broken: u32,
    pins_broken: u32,
    out_of_pins: u32,
    tries: u32,
    left: u32,
}

/// `NVF_PLAY_TEST_LOCKPICK`.
#[derive(Resource, Default)]
struct LockpickAid {
    steps: Vec<(f32, AidStep)>,
    skill: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AidStep {
    Open(u32),
    Skill(f32),
    Input(LockpickInput),
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let (op, arg) = what.split_once(':').unwrap_or((what, ""));
            let num = |name: &str| arg.trim().parse::<f32>().map_err(|_| format!("{what:?}: expected {op}:<{name}>"));
            let step = match op {
                "open" => AidStep::Open(u32::from_str_radix(arg.trim().trim_start_matches("0x"), 16).map_err(|_| format!("{what:?}: expected open:<reference hex>"))?),
                "skill" => AidStep::Skill(num("n")?),
                "pin" => AidStep::Input(LockpickInput::Pin(num("degrees")?)),
                "spot" => AidStep::Input(LockpickInput::Spot),
                "off" => AidStep::Input(LockpickInput::Off(num("degrees")?)),
                "torque" => AidStep::Input(LockpickInput::Torque(num("seconds")?)),
                "force" if arg.is_empty() => AidStep::Input(LockpickInput::Force(None)),
                "force" => AidStep::Input(LockpickInput::Force(Some(num("roll")?))),
                "exit" => AidStep::Input(LockpickInput::Exit),
                "help" => AidStep::Input(LockpickInput::CloseHelp),
                _ => return Err(format!("{what:?}: expected open:, skill:, pin:, spot, off:, torque:, force, help or exit")),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct LockpickPlugin;

impl Plugin for LockpickPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_os = "android")]
        touch::install(app);
        let steps = match std::env::var("NVF_PLAY_TEST_LOCKPICK").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_LOCKPICK: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        let debug = crate::dialogue::debug_switch(std::env::var("NVF_PLAY_DEBUG_LOCKPICK").ok().as_deref());
        if debug {
            info!("debug switch NVF_PLAY_DEBUG_LOCKPICK: the lockpicking menu shows its debug meter");
        }
        app.init_resource::<LockpickScreen>()
            .init_resource::<PracticeLock>()
            .init_resource::<LockpickStats>()
            .init_resource::<PointerOverLock>()
            .insert_resource(LockpickDebug(debug))
            .insert_resource(LockpickAid { steps, skill: None })
            .add_message::<OpenLockpick>()
            .add_message::<LockpickInput>()
            .add_message::<LockOpened>();
        #[cfg(target_os = "android")]
        app.add_systems(
            Update,
            (run_aid, open, (touch::input, keys, buttons, apply, step).chain().run_if(in_state(PlayState::Menu)), pointer, screen::draw, screen::draw_debug, screen::fade)
                .chain()
                .after(crate::menu::open_menus)
                .in_set(PlaySet::Ui),
        )
        .add_systems(OnExit(PlayState::Menu), (close, touch::clear));
        #[cfg(not(target_os = "android"))]
        app.add_systems(
            Update,
            (run_aid, open, (keys, buttons, apply, step).chain().run_if(in_state(PlayState::Menu)), pointer, screen::draw, screen::draw_debug, screen::fade)
                .chain()
                .after(crate::menu::open_menus)
                .in_set(PlaySet::Ui),
        )
        .add_systems(OnExit(PlayState::Menu), close);
        app
            // The help box's scrolling, for every menu that shows it.
            .add_systems(Update, help::scroll.in_set(PlaySet::Ui))
            .add_systems(Last, report);
    }
}

fn run_aid(time: AidTime, mut aid: ResMut<LockpickAid>, mut open: MessageWriter<OpenLockpick>, mut input: MessageWriter<LockpickInput>) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).copied().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_LOCKPICK: {step:?}");
        match step {
            AidStep::Open(r) => {
                open.write(OpenLockpick { reference: r });
            }
            AidStep::Skill(s) => aid.skill = Some(s),
            AidStep::Input(i) => {
                input.write(i);
            }
        }
    }
}

/// A seed: different every time [design: where the sweet spot lies is not known to be
/// kept per lock, §3].
fn seed(reference: FormId, now: f32) -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    t ^ ((reference.0 as u64) << 24) ^ now.to_bits() as u64
}

/// 0 to 1 from the generator.
fn unit(rng: &mut ChaCha8Rng) -> f32 {
    (rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
}

/// The line shown when the menu does not open [hypothesis: the original's words are its
/// own settings, not in the plugins; these are NVForever's], in the chosen language
/// (`lang/*.toml`, #3146).
pub fn refusal_line(r: Refusal) -> String {
    match r {
        Refusal::RequiresKey => tr("This lock requires a key.").into(),
        Refusal::Broken => tr("This lock is broken. It can only be opened with a key.").into(),
        Refusal::Skill { needed } => trf("You need a Lockpick skill of {needed} to pick this lock.", &[("needed", &format!("{needed:.0}"))]),
        Refusal::NoPins => tr("You have no bobby pins.").into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn open(
    (mut requests, mut practice): (MessageReader<OpenLockpick>, ResMut<PracticeLock>),
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<LockpickScreen>,
    paused: Res<Paused>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    aid: Res<LockpickAid>,
    hints: Option<Res<touch::Hints>>,
    time: AidTime,
    (mut stats, mut corner, mut sounds, bindings): (ResMut<LockpickStats>, Option<ResMut<crate::corner_messages::CornerMessages>>, MessageWriter<UiSound>, Res<Bindings>),
) {
    let in_world = *state.get() == PlayState::Playing && screen.0.is_none() && !paused.0;
    // A practice lock waits for the console to have closed.
    let (asked, practice) = match requests.read().last().copied() {
        Some(o) => (o, None),
        None => match practice.0.take_if(|_| in_world) {
            Some(level) => (OpenLockpick { reference: 0 }, Some(level)),
            None => return,
        },
    };
    stats.asked += 1;
    if !in_world {
        info!("lockpick: request for {:08X} ignored, the player is not in the world", asked.reference);
        return;
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else {
        warn!("lockpick: scripts are not running; lockpicking needs the game's rules");
        return;
    };
    let reference = FormId(asked.reference);
    let name = match practice {
        Some(_) => tr("practice lock").to_string(),
        None => sim.base_of(asked.reference).and_then(|b| sim.name(b)).map_or_else(|| reference.to_string(), str::to_string),
    };
    let now = time.elapsed_secs();
    let (opening, skill) = with_world(rt, &mut sim, |rt| {
        let skill = aid.skill.unwrap_or_else(|| rules::skill(&mut rt.game));
        let opening = match practice {
            Some(level) => match rules::pins(&mut rt.game) {
                n if n <= 0 => Opening::Refused(Refusal::NoPins),
                pins => Opening::Pick { level, skill, pins },
            },
            None => rules::opening(&mut rt.game, reference, skill),
        };
        (opening, skill)
    });
    match opening {
        Opening::NotLocked => info!("lockpick: {reference} {name} is not locked"),
        Opening::Refused(r) => {
            stats.refused += 1;
            let line = refusal_line(r);
            // The player's tweak names the key a lock needs [design: docs/design/tweaks.md].
            let key = matches!(r, Refusal::RequiresKey).then(|| rt.game.tweaks.lock_needs_key_name).unwrap_or(false).then(|| rt.game.world.peek(reference).and_then(|s| s.lock).and_then(|l| l.key)).flatten();
            // The player's tweak says the player's own skill [design: docs/design/tweaks.md].
            let line = if matches!(r, Refusal::Skill { .. }) && rt.game.tweaks.show_current_skill { trf("{line} (yours: {skill})", &[("line", &line.trim_end_matches('.')), ("skill", &format!("{skill:.0}"))]) } else { line };
            let line = match key.and_then(|k| sim.name(k)) {
                Some(k) => format!("{}: {k}.", line.trim_end_matches('.')),
                None => line,
            };
            info!("lockpick: {reference} {name} refused (Lockpick {skill:.0}): on screen: {line:?}");
            // A top-left corner message (owner, list 5 item 45).
            if let Some(c) = corner.as_deref_mut() {
                c.push(line.into());
            }
            sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::Refused });
        }
        Opening::Pick { level, skill, pins } => {
            stats.opened += 1;
            let mut rng = ChaCha8Rng::seed_from_u64(seed(reference, now));
            // The player's tweak: a force chance that grows near the sweet spot [design: docs/design/tweaks.md].
            let settings = Settings { force_near_bonus: rt.game.tweaks.lockpick_force_near_bonus, force_spares_lock: rt.game.tweaks.force_spares_lock, ..Settings::default() };
            let mut game = Picking::new(level, skill, pins, unit(&mut rng), settings);
            // The player's tweak: the pin in the lock is as worn as the last one left
            // (kept in the save) [design: docs/design/tweaks.md].
            if rt.game.tweaks.remember_pin_health {
                if let Some(h) = rt.game.extra.get(&FormId::PLAYER_REF).and_then(|e| e.pin_health) {
                    game.health = h;
                }
            }
            info!(
                "lockpick: open {reference} {name}: {} lock (Lockpick {skill:.0}, {pins} bobby pins): sweet spot {:.1}° ± {:.1}°, Force Lock {:.0}%",
                level.name(),
                game.sweet,
                game.half,
                game.force_chance()
            );
            rt.game.open_menu(MENU_LOCKPICK);
            let help = hints.as_ref().is_none_or(|h| h.0)
                .then(|| rules::first_help(&mut rt.game)).flatten()
                .map(|(title, text)| Help::new(&rt.game, title, &text, &bindings));
            if let Some(h) = &help {
                info!("lockpick: help message {} {:?} shown (first use): {:?}", rules::HELP_MESSAGE, h.title, h.text.lines().next().unwrap_or(""));
            }
            screen.0 = Some(Session {
                reference,
                practice: practice.is_some(),
                name,
                game,
                status: None,
                opened: now,
                pin_clip: PinClip::In,
                pin_since: now,
                ended: None,
                help,
                words: Words::read(&rt.game),
                leaving: None,
                moved: 0.0,
                aid_torque_until: None,
            });
            sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::Opened });
            next.set(PlayState::Menu);
        }
    }
}

/// The keys and the mouse.
#[allow(clippy::too_many_arguments)]
fn keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    time: AidTime,
    screen: Res<LockpickScreen>,
    over_button: Query<&PointerState, With<screen::LockButton>>,
    mut out: MessageWriter<LockpickInput>,
    input: ControlInput,
    script: Option<Res<ScriptState>>,
) {
    let keys = keys.snapshot();
    let Some(s) = screen.0.as_ref() else { return };
    if s.leaving.is_some() {
        return;
    }
    // The help box is on top: E closes it (Enter, Esc and Tab too [design]).
    if s.help.is_some() {
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab, KeyCode::Enter]) {
            out.write(LockpickInput::CloseHelp);
        }
        return;
    }
    if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab]) {
        out.write(LockpickInput::Exit);
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        out.write(LockpickInput::Force(None));
    }
    let dt = time.delta_secs_f64() as f32;
    let mut turn = 0.0;
    if keys.pressed(KeyCode::ArrowLeft) {
        turn -= KEY_DEGREES * dt;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        turn += KEY_DEGREES * dt;
    }
    // The player's tweak: the strafe keys move the pin, Shift faster and Ctrl slower
    // [design: docs/design/tweaks.md].
    if moving_keys(script.as_deref()) && (!input.controller() || crate::bindings::keys_with_pad()) {
        let k = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
            2.0
        } else if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
            0.5
        } else {
            1.0
        };
        if input.physical_pressed(Control::SlideLeft) {
            turn -= KEY_DEGREES * k * dt;
        }
        if input.physical_pressed(Control::SlideRight) {
            turn += KEY_DEGREES * k * dt;
        }
    }
    // The pointer on a button moves no pin [design: the original hides the pointer over
    // the lock only, `LPM_DetectArea`].
    let on_button = over_button.iter().any(|i| *i != PointerState::None);
    if let Some(m) = motion.filter(|_| !on_button) {
        turn += m.delta.x * MOUSE_DEGREES;
    }
    if turn != 0.0 {
        out.write(LockpickInput::Turn(turn));
    }
}

/// All ordinary angle devices share the same rule-gated movement/sound accounting.
fn turn_pin(game: &mut Picking, moved: &mut f32, delta: f32) {
    if !delta.is_finite() { return; }
    let at = game.pin;
    if game.set_pin(at + delta) { *moved += (game.pin - at).abs(); }
}

/// Whether torque is held: any of the four movement controls [documented: the help
/// message]; with the player's Keyboard Movement tweak Forward, Back and Jump, the strafe
/// keys moving the pin instead [design: docs/design/tweaks.md].
fn torque_held(input: &ControlInput, moving_keys: bool) -> bool {
    let controls: &[Control] = if moving_keys { &[Control::Forward, Control::Back, Control::Jump] } else { &[Control::Forward, Control::Back, Control::SlideLeft, Control::SlideRight] };
    controls.iter().any(|&c| input.pressed(c))
}

/// A controller's optional keyboard contribution uses real key/mouse provenance;
/// its angle stick must never be mistaken for a physical torque key.
fn physical_torque_held(input: &ControlInput, moving_keys: bool) -> bool {
    let controls: &[Control] = if moving_keys { &[Control::Forward, Control::Back, Control::Jump] } else { &[Control::Forward, Control::Back, Control::SlideLeft, Control::SlideRight] };
    controls.iter().any(|&c| input.physical_pressed(c))
}

/// The Keyboard Movement tweak is on.
fn moving_keys(script: Option<&ScriptState>) -> bool {
    script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.lockpick_move_keys)
}

fn buttons(screen: Res<LockpickScreen>, pressed: Query<(&PointerState, &screen::LockButton), Changed<PointerState>>, mut out: MessageWriter<LockpickInput>) {
    if screen.0.is_none() {
        return;
    }
    for (i, b) in &pressed {
        if *i == PointerState::Pressed {
            out.write(b.0);
        }
    }
}

/// Whether the pointer is over the lock: the menu open, no help box up, not leaving, and
/// the pointer in the menu file's detect area (`screen::in_detect_area`).
fn pointer(screen: Res<LockpickScreen>, windows: Query<&Window, With<PrimaryWindow>>, mut over: ResMut<PointerOverLock>) {
    let picking = screen.0.as_ref().is_some_and(|s| s.help.is_none() && s.leaving.is_none());
    let hide = picking
        && windows.single().ok().and_then(|w| Some(screen::in_detect_area(w.cursor_position()?, Vec2::new(w.width(), w.height())))).unwrap_or(false);
    if over.0 != hide {
        over.0 = hide;
    }
}

/// Close the menu: back to the world.
fn leave(next: &mut NextState<PlayState>, why: &str) {
    info!("lockpick: closed ({why})");
    next.set(PlayState::Playing);
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut input: MessageReader<LockpickInput>,
    mut screen: ResMut<LockpickScreen>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    time: AidTime,
    mut stats: ResMut<LockpickStats>,
    mut rng: Local<Option<ChaCha8Rng>>,
    (mut sounds, mut opened): (MessageWriter<UiSound>, MessageWriter<LockOpened>),
) {
    let now = time.elapsed_secs();
    for &cmd in input.read() {
        let Some(s) = screen.0.as_mut() else { return };
        let g = &mut s.game;
        match cmd {
            LockpickInput::CloseHelp => {
                if s.help.take().is_some() {
                    info!("lockpick: help message closed");
                }
            }
            _ if s.leaving.is_some() => {}
            LockpickInput::Exit => {
                if s.ended.is_some() {
                    s.begin_leaving(now, "after the game");
                } else {
                    stats.left += 1;
                    if g.phase == Phase::Picking {
                        s.pin(PinClip::Out, now);
                    }
                    s.begin_leaving(now, "the player left");
                }
            }
            // The help box is on top of the menu until it is closed.
            _ if s.ended.is_some() || s.help.is_some() => {}
            LockpickInput::Turn(d) => turn_pin(g, &mut s.moved, d),
            LockpickInput::Pin(a) => {
                g.set_pin(a);
                info!("lockpick: pin at {:.1}° (sweet spot {:.1}° ± {:.1}°): the lock turns {:.0}°", g.pin, g.sweet, g.half, g.max_turn());
            }
            LockpickInput::Spot => {
                g.set_pin(g.sweet);
                info!("lockpick: pin at {:.1}°, in the sweet spot: the lock turns {:.0}°", g.pin, g.max_turn());
            }
            LockpickInput::Off(d) => {
                // To the side with room for it.
                let right = g.sweet + g.half + d;
                let a = if right <= PIN_RANGE { right } else { g.sweet - g.half - d };
                g.set_pin(a);
                info!("lockpick: pin at {:.1}°, {d:.1}° off the sweet spot ({:.1}° ± {:.1}°): the lock turns {:.0}°", g.pin, g.sweet, g.half, g.max_turn());
            }
            LockpickInput::Torque(secs) => s.aid_torque_until = Some(now + secs),
            LockpickInput::Force(roll) => {
                if g.phase != Phase::Picking {
                    continue;
                }
                let reference = s.reference;
                let roll = roll.unwrap_or_else(|| 100.0 * unit(rng.get_or_insert_with(|| ChaCha8Rng::seed_from_u64(seed(reference, now)))));
                let chance = g.force_chance();
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
                match g.force(roll) {
                    Some(true) => {
                        stats.forced += 1;
                        let level = g.level;
                        info!("lockpick: Force Lock ({chance:.0}%, roll {roll:.1}): {reference} forced open");
                        if !s.practice {
                            let owner = with_world(rt, &mut sim, |rt| rules::opened(&mut rt.game, reference, level, false));
                            crime_hook(&mut opened, reference, owner, true);
                        }
                        s.status = Some(tr("Lock forced open").into());
                        s.ended = Some(now);
                        sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::Unlocked });
                    }
                    // The player's tweak: the lock holds, the pin broke [design: docs/design/tweaks.md].
                    Some(false) if g.phase != Phase::LockBroken => {
                        stats.pins_broken += 1;
                        with_world(rt, &mut sim, |rt| rules::pin_broke(&mut rt.game));
                        info!("lockpick: Force Lock ({chance:.0}%, roll {roll:.1}): failed, the pin broke and the lock holds ({} left, tweak)", g.pins);
                        s.status = Some(if g.pins > 0 { tr("The bobby pin broke").into() } else { tr("You have no bobby pins left").into() });
                        if g.pins == 0 {
                            s.ended = Some(now);
                        }
                        s.pin(PinClip::Break, now);
                        sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::PinBroke });
                    }
                    Some(false) => {
                        stats.broken += 1;
                        if !s.practice {
                            with_world(rt, &mut sim, |rt| rules::broke(&mut rt.game, reference));
                        }
                        info!("lockpick: Force Lock ({chance:.0}%, roll {roll:.1}): {reference} broken for good, the pin with it ({} left)", g.pins);
                        s.status = Some(tr("The lock is broken").into());
                        s.ended = Some(now);
                        s.pin(PinClip::Break, now);
                        sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::ForceFailed });
                    }
                    None => {}
                }
            }
        }
    }
}

fn crime_hook(out: &mut MessageWriter<LockOpened>, reference: FormId, owner: Option<FormId>, forced: bool) {
    match owner {
        Some(o) => info!("lockpick: crime hook: {reference} is owned by {o} ({}; judged by `crate::crime`)", if forced { "forced" } else { "picked" }),
        None => info!("lockpick: crime hook: {reference} has no owner"),
    }
    out.write(LockOpened { reference: reference.0, owner: owner.map(|o| o.0), how: if forced { OpenedBy::Forced } else { OpenedBy::Picked } });
}

/// The game's frame: torque, the lock turning, wear, breaks, the end.
#[allow(clippy::too_many_arguments)]
fn step(
    input: ControlInput,
    held: Option<Res<touch::HeldTorque>>,
    time: AidTime,
    mut screen: ResMut<LockpickScreen>,
    mut next: ResMut<NextState<PlayState>>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    mut stats: ResMut<LockpickStats>,
    (mut sounds, mut opened): (MessageWriter<UiSound>, MessageWriter<LockOpened>),
) {
    let Some(s) = screen.0.as_mut() else { return };
    let now = time.elapsed_secs();
    // The player's tweak runs the lock at the world clock's speed (a script's
    // SetGlobalTimeMultiplier) [design: docs/design/tweaks.md].
    let speed = script.as_deref().and_then(|s| s.rt.as_ref()).filter(|rt| rt.game.tweaks.minigames_timescale).map_or(1.0, |rt| rt.game.ui.time_multiplier.unwrap_or(1.0).max(0.0) as f32);
    let dt = time.delta_secs_f64() as f32 * speed;
    if let Some((since, why)) = s.leaving {
        if now - since >= LEAVE_SECONDS {
            leave(&mut next, why);
        }
        s.game.step(dt, false);
        return;
    }
    if let Some(end) = s.ended {
        if now - end >= END_SECONDS {
            s.begin_leaving(now, "the game is over");
        }
        // The lock springs back and a broken pin's wait runs on, without torque.
        s.game.step(dt, false);
        return;
    }
    // Nothing is picked while the help box is up.
    if s.help.is_some() {
        s.game.step(dt, false);
        return;
    }
    let mut ui = |event| {
        sounds.write(UiSound { menu: UiMenu::Lockpick, event });
    };
    if s.moved >= MOVE_SOUND_DEGREES {
        s.moved = 0.0;
        ui(UiEvent::PinMoved);
    }
    let aid = s.aid_torque_until.is_some_and(|t| now < t);
    if s.aid_torque_until.is_some() && !aid {
        s.aid_torque_until = None;
    }
    // Keep the selected controller's stick for angle and its trigger for torque.
    // Main's keyboard/mouse tweaks join that owner only with the explicit opt-in.
    let mouse_turns = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.lockpick_mouse_turn);
    let moving = moving_keys(script.as_deref());
    let physical = torque_held(&input, moving) || (mouse_turns && input.mouse.pressed(MouseButton::Left));
    let torque = if let Some(held) = held.as_deref() {
        let keyboard_with_pad = input.controller() && crate::bindings::keys_with_pad()
            && (physical_torque_held(&input, moving) || (mouse_turns && input.physical_mouse_pressed(MouseButton::Left)));
        touch::torque(physical, input.controller(), held, aid) || keyboard_with_pad
    } else {
        physical || aid
    };
    let strained = s.game.stopped;
    let reference = s.reference;
    let events = s.game.step(dt, torque);
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.tweaks.remember_pin_health) {
        rt.game.extra.entry(FormId::PLAYER_REF).or_default().pin_health = Some(s.game.health);
    }
    for e in events {
        match e {
            rules::Event::Turning => {
                stats.tries += 1;
                info!("lockpick: torque on, pin at {:.1}° (the lock can turn {:.0}°)", s.game.pin, s.game.max_turn());
                ui(UiEvent::Turning);
            }
            rules::Event::Stopped { at } => {
                info!("lockpick: the lock stopped at {at:.0}°: the pin is under strain");
                ui(UiEvent::Stopped);
                ui(UiEvent::Strain { on: true });
            }
            rules::Event::PinBroke { left } => {
                stats.pins_broken += 1;
                if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                    with_world(rt, &mut sim, |rt| rules::pin_broke(&mut rt.game));
                }
                info!("lockpick: the bobby pin broke, {left} left");
                ui(UiEvent::Strain { on: false });
                ui(UiEvent::PinBroke);
                s.pin(PinClip::Break, now);
                if left == 0 {
                    stats.out_of_pins += 1;
                    s.status = Some(tr("You have no bobby pins left").into());
                    s.ended = Some(now);
                } else {
                    s.status = Some(tr("Your bobby pin broke").into());
                }
            }
            rules::Event::NewPin => {
                info!("lockpick: a new bobby pin is in");
                s.pin(PinClip::In, now);
                s.status = None;
            }
            rules::Event::Opened => {
                stats.picked += 1;
                let level = s.game.level;
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                if s.practice {
                    info!("lockpick: the lock turned all the way: practice lock picked open ({} lock)", level.name());
                } else {
                    let xp_before = rt.game.player.xp;
                    let owner = with_world(rt, &mut sim, |rt| rules::opened(&mut rt.game, reference, level, true));
                    info!("lockpick: the lock turned all the way: {reference} picked open ({} lock, {:.0} XP)", level.name(), rt.game.player.xp - xp_before);
                    crime_hook(&mut opened, reference, owner, false);
                }
                if strained {
                    ui(UiEvent::Strain { on: false });
                }
                ui(UiEvent::Unlocked);
                s.status = Some(tr("Lock picked").into());
                s.ended = Some(now);
            }
        }
    }
    // Torque let go against the stop: the strain is off.
    if strained && !s.game.stopped && s.game.phase == Phase::Picking {
        ui(UiEvent::Strain { on: false });
    }
}

fn close(
    mut screen: ResMut<LockpickScreen>,
    script: Option<ResMut<ScriptState>>,
    mut sounds: MessageWriter<UiSound>,
    next: Option<ResMut<crate::activate::ActivateNext>>,
    sim: Option<Res<SimState>>,
) {
    let Some(s) = screen.0.take() else { return };
    // Picked or forced open: the door or container is used as if the player activated
    // it (playtest list 5 item 64; `crate::activate::ActivateNext`).
    // The player's tweaks can leave the door or the container shut after the pick [design: docs/design/tweaks.md].
    let tw = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| (rt.game.tweaks.no_door_after_pick, rt.game.tweaks.no_container_after_pick));
    let base = sim.as_ref().and_then(|sim| sim.base_of(s.reference.0).and_then(|b| sim.forms.get(b)).map(|b| (matches!(b.data, fnv_sim::forms::BaseData::Door(_)), matches!(b.data, fnv_sim::forms::BaseData::Container(_)))));
    let held = matches!((tw, base), (Some((true, _)), Some((true, _))) | (Some((_, true)), Some((_, true))));
    if held {
        info!("lockpick: {} stays shut after the pick (tweak)", s.reference);
    }
    if s.game.phase == Phase::Open && !s.practice && !held {
        if let Some(mut n) = next {
            n.0 = Some(s.reference.0);
        }
    }
    if s.game.stopped {
        sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::Strain { on: false } });
    }
    sounds.write(UiSound { menu: UiMenu::Lockpick, event: UiEvent::Closed });
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        if rt.game.menu == Some(MENU_LOCKPICK) {
            rt.game.close_menu();
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<LockpickStats>) {
    if exit.read().next().is_none() || stats.asked == 0 {
        return;
    }
    let s = &*stats;
    info!(
        "lockpick summary: {} asked, {} opened ({} refused); {} picked, {} forced, {} broken by forcing; {} torque tries, {} pins broken, {} times out of pins; {} left before the end",
        s.asked, s.opened, s.refused, s.picked, s.forced, s.broken, s.tries, s.pins_broken, s.out_of_pins, s.left
    );
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> LockpickScreen {
    let mut game = Picking::new(rules::Level::Easy, 50.0, 10, 0.5, Settings::default());
    if step >= 1 {
        game.set_pin(100.0);
    }
    if step >= 2 {
        game.turn = 10.0;
    }
    if step >= 3 {
        game.pins -= 1;
    }
    if step >= 4 {
        game.phase = Phase::PinBroken { since: step as f32 };
    }
    LockpickScreen(Some(Session {
        reference: FormId(42),
        practice: true,
        name: "Synthetic lock".into(),
        game,
        status: (step >= 5).then(|| "Synthetic status".into()),
        opened: step as f32,
        pin_clip: PinClip::In,
        pin_since: step as f32,
        ended: None,
        help: (step >= 6).then(|| Help {
            title: "Synthetic help".into(),
            text: String::new(),
            words: help::BoxWords { manual: String::new(), close: String::new(), close_hint: String::new() },
        }),
        words: Words { skill: String::new(), picks: String::new(), level: String::new(), exit: String::new(), force_hint: String::new(), exit_hint: String::new() },
        leaving: (step >= 7).then_some((step as f32, "synthetic")),
        moved: step as f32,
        aid_torque_until: Some(step as f32),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:skill:30,2:open:0010B2D4,3:off:20,4:torque:2.5,5:spot,6:pin:45,7:force,8:force:99,9:exit,10:help").unwrap();
        assert_eq!(s[9], (10.0, AidStep::Input(LockpickInput::CloseHelp)));
        assert_eq!(s[0], (1.0, AidStep::Skill(30.0)));
        assert_eq!(s[1], (2.0, AidStep::Open(0x0010B2D4)));
        assert_eq!(s[2], (3.0, AidStep::Input(LockpickInput::Off(20.0))));
        assert_eq!(s[3], (4.0, AidStep::Input(LockpickInput::Torque(2.5))));
        assert_eq!(s[4], (5.0, AidStep::Input(LockpickInput::Spot)));
        assert_eq!(s[5], (6.0, AidStep::Input(LockpickInput::Pin(45.0))));
        assert_eq!(s[6], (7.0, AidStep::Input(LockpickInput::Force(None))));
        assert_eq!(s[7], (8.0, AidStep::Input(LockpickInput::Force(Some(99.0)))));
        assert_eq!(s[8], (9.0, AidStep::Input(LockpickInput::Exit)));
        assert!(parse_aid("1:pick").is_err());
        assert!(parse_aid("1:torque").is_err());
    }

    #[test]
    fn refusals_say_why() {
        assert_eq!(refusal_line(Refusal::Skill { needed: 50.0 }), "You need a Lockpick skill of 50 to pick this lock.");
        assert!(refusal_line(Refusal::NoPins).contains("bobby pins"));
        assert!(refusal_line(Refusal::Broken).contains("key"));
    }

    #[test]
    fn the_random_spot_is_in_range() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        for _ in 0..1000 {
            let u = unit(&mut rng);
            assert!((0.0..1.0).contains(&u));
        }
    }
    include!("lockpick/input_union_tests.rs");
}
