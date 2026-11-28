//! The Wait menu, and the same menu as a Sleep menu while the player lies on a bed
//! (docs/spec-notes/wait-menu.md).
//!
//! The Wait control (`Control::Rest`, T by default) opens it while playing. The player
//! picks 1 to 24 hours on the slider and accepts; game time then passes one hour at a
//! time ([`HOUR_SECONDS`] apart) on the one play clock, the simulation's calendar
//! (`fnv_sim::World::advance_hours`: the GameHour, GameDaysPassed and date globals), so
//! the sky, the lighting, the AI packages' clock and scripts read each new hour. The menu
//! counts the hours down meanwhile and closes after the last one; Cancel stops the wait
//! with the hours passed so far.
//!
//! **Sleeping fades the screen out** [owner request 2026-10-11]: once a sleep's hours are
//! accepted the world behind the menu goes to black over [`FADE_SECONDS`] ([`SleepFade`],
//! which whoever draws the world applies), the menu staying on top with its count; when
//! the menu closes the world comes back as fast. A wait does not fade.
//!
//! It is a `PlayState::Menu` like the container and inventory menus: the player, the
//! world and the running game clock stand still, the cursor is free, and the picture
//! follows the menu pause switch (`crate::pause`). To scripts it is menu 1012
//! ([`crate::script::MENU_SLEEP_WAIT`]). It stays shut while a script has the Pip-Boy
//! group of the player's controls off (docs/spec-notes/controls.md §5).
//!
//! Keys: Left / Right (or A / D) and the mouse wheel move the slider one hour, Page Up /
//! Page Down six, Home and End go to its ends, E or Enter accepts, X, Esc or the Wait
//! control cancels; the slider and the two buttons take the mouse (wait-menu.md §6 says
//! which of these are the original's). [`model`] is the menu as plain data.
//!
//! **Refusals** (wait-menu.md §9): instead of opening, a line on the screen says why the
//! player cannot wait or sleep now ([`refusal`]): a script forbade waiting, the player is
//! swimming or in the air, takes radiation damage (an empty check yet), is in combat, has
//! hostile actors near, trespasses, or the bed is someone else's. A script that opens the
//! menu without asking for the check gets it regardless.
//!
//! **Scripts** see a sleep or a wait while its hours pass (`fnv_game::rest`:
//! `IsPCSleeping`, `IsTimePassing`), can open the menu (`ShowSleepWaitMenu`), make the
//! player sleep (`SetPCSleepHours`) and stop a sleep (`WakeUpPC`); [`script_requests`]
//! takes what they asked. A bed's own script is told of the sleep
//! (`Runtime::bed_activated_for_sleep`): that is what gives Well Rested.
//! A trip's loading screen that comes up while the menu is open passes the hours left
//! at once behind it and closes the menu ([`pass_behind_screen`]; the raft to the Fort
//! asks for its sleep in the frame it moves the player, issue #2281).
//!
//! Test aid `NVF_PLAY_TEST_WAIT=<seconds>:<what>[,…]`: at each time `open` presses the
//! Wait control, `hours=<n>` sets the slider, `accept` and `cancel` press the buttons.
//! An `open` due while the player is not in the world (a start-up message box is up)
//! waits until they are, and the steps after it come as much later ([`run_aid`]).
//! For the mouse: `point=<x>:<y>` puts the pointer there (parts of the window's width
//! and height, 0 to 1), `press` and `release` are its left button; they go the way the
//! real pointer does (Bevy's UI finds what is under it).
//! At exit, when the menu was opened at all: "wait summary: …".
//!
//! With `[UI] TileMenus=SleepWaitMenu` (T15b) the menu is drawn from the game's
//! `menus\sleep_wait_menu.xml` (`crate::tiles::wait_menu`): the hand-built drawing below
//! is not spawned, and its keys and the mouse wheel stay the menu's.

pub mod model;

use crate::bindings::{Control, ControlInput};
use crate::control::{group, ControlLocks, LockStats};
use crate::furniture::PlayerSeat;
use crate::menu::ActiveMenu;
use crate::pause::{AidTime, Paused};
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::{PlaySet, PlayState};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use bevy::ui::RelativeCursorPosition;
use fnv_game::rest::RestRequest;
use fnv_sim::FormId;
use model::{Command, Model, Refusal, RefusalWords, Words, JUMP, MAX_HOURS};

/// Real seconds between two hours passing [engineering choice: the original's pace is
/// not captured; a full day takes twelve seconds].
pub const HOUR_SECONDS: f32 = 0.5;

/// Real seconds between two hours passing under Faster Sleep and Wait (`bFasterSleepWait`)
/// [design: 24 hours pass in 3.6 seconds instead of 12 seconds].
pub const FASTER_HOUR_SECONDS: f32 = 0.15;

/// Active seconds per hour for the Sleep / Wait countdown (`HOUR_SECONDS` when disabled,
/// `FASTER_HOUR_SECONDS` when `bFasterSleepWait` is enabled) [design].
pub const fn hour_seconds(faster: bool) -> f32 {
    if faster { FASTER_HOUR_SECONDS } else { HOUR_SECONDS }
}

/// Seconds the screen takes to go black when a sleep is accepted, and to come back
/// after it [design].
pub const FADE_SECONDS: f32 = 1.0;

/// How far the world is faded to black by sleeping: 0 not at all, 1 black.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct SleepFade(pub f32);

/// The fade `dt` seconds on: towards black while a sleep's hours pass, back otherwise.
pub fn fade_step(fade: f32, sleeping: bool, dt: f32) -> f32 {
    let step = dt.max(0.0) / FADE_SECONDS;
    if sleeping { (fade + step).min(1.0) } else { (fade - step).max(0.0) }
}

/// The open Wait or Sleep menu, if any.
#[derive(Resource, Default)]
pub struct WaitMenu(pub Option<Model>);

impl WaitMenu {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

/// Open the menu. The default is the player's own request (the Wait control, lying down
/// on a bed, the test aid).
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct OpenWait {
    /// A script's choice of the Sleep or the Wait menu (`ShowSleepWaitMenu`); `None`:
    /// the Sleep menu when the player lies on a bed.
    pub sleep: Option<bool>,
    /// A script opens it without the check of the conditions: no refusal applies.
    pub unchecked: bool,
    /// These hours start passing at once (`SetPCSleepHours`).
    pub hours: Option<u32>,
}

/// Outdoors and indoors, how near a hostile actor refuses a wait or a sleep, game units.
/// The original reads the game settings `fHostileActorExteriorDistance` and
/// `fHostileActorInteriorDistance` [documented: GECK wiki IsHostilesNearby]; neither has
/// a record in FalloutNV.esm and no source read gives their values, so these two numbers
/// are NVForever's [engineering choice] until someone reads them off the original
/// (`GetGS` in its console). A plugin that sets the settings is followed.
pub const HOSTILE_DISTANCE: [f32; 2] = [2048.0, 1024.0];

/// An accepted hour has passed: the calendar is one hour on (whoever moves the world's
/// people moves them on, `nvforever`'s residents).
#[derive(Message, Clone, Copy, Debug)]
pub struct HourPassed {
    pub sleep: bool,
}

/// A wait or a sleep ended (its hours all passed, or Cancel after at least one) with
/// the hours that passed; `crate::save` autosaves on it.
#[derive(Message, Clone, Copy, Debug)]
pub struct RestEnded {
    pub sleep: bool,
    pub hours: u32,
}

/// A command for the open menu (keys, mouse, test aid).
#[derive(Message, Clone, Copy, Debug)]
pub struct WaitInput(pub Command);

/// For the run summary.
#[derive(Resource, Default, Debug)]
struct WaitStats {
    opened: u32,
    as_sleep: u32,
    hours: u32,
    refused: u32,
    /// Requests answered with a refusal line, by [`Refusal`].
    turned_down: [u32; Refusal::COUNT],
    /// Sleeps a bed's own script was told of.
    beds_told: u32,
}

/// `NVF_PLAY_TEST_WAIT`: what to do when.
#[derive(Resource, Default)]
struct WaitAid(Vec<(f32, AidStep)>);

#[derive(Debug, Clone, Copy, PartialEq)]
enum AidStep {
    Open,
    Input(Command),
    Point(f32, f32),
    Press,
    Release,
}

impl AidStep {
    fn of_the_pointer(&self) -> bool {
        matches!(self, AidStep::Point(..) | AidStep::Press | AidStep::Release)
    }
}

/// The steps of `aid` due at `now` that are (or are not) the pointer's, in time order.
fn due_steps(aid: &mut WaitAid, now: f32, pointer: bool) -> Vec<AidStep> {
    let mine = |(t, s): &(f32, AidStep)| *t <= now && s.of_the_pointer() == pointer;
    let mut due: Vec<(f32, AidStep)> = aid.0.iter().filter(|e| mine(e)).copied().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.0.retain(|e| !mine(e));
    due.into_iter().map(|(_, s)| s).collect()
}

/// An `open` of `aid` due at `now` while the player is not in the world is held: it and
/// every step after it come later, so that they stay as far apart as asked. Returns the
/// steps due before it (they go on) and how much later the rest come, if an `open` was
/// due.
fn hold_open(aid: &mut WaitAid, now: f32) -> Option<(Vec<AidStep>, f32)> {
    let t = aid.0.iter().filter(|(t, s)| *s == AidStep::Open && *t <= now).map(|e| e.0).min_by(f32::total_cmp)?;
    let before = due_steps(aid, t.next_down(), false);
    let by = now - t;
    aid.0.iter_mut().filter(|e| e.0 >= t).for_each(|e| e.0 += by);
    Some((before, by))
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let step = match what {
                "open" => AidStep::Open,
                "accept" => AidStep::Input(Command::Accept),
                "cancel" => AidStep::Input(Command::Cancel),
                "press" => AidStep::Press,
                "release" => AidStep::Release,
                _ => {
                    let hours = what.strip_prefix("hours=").and_then(|n| n.trim().parse::<u32>().ok());
                    let point = what.strip_prefix("point=").and_then(|p| p.split_once(':')).and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)));
                    match (hours, point) {
                        (Some(n), _) => AidStep::Input(Command::Hours(n)),
                        (_, Some((x, y))) => AidStep::Point(x, y),
                        _ => return Err(format!("{what:?}: expected open, hours=<n>, accept, cancel, point=<x>:<y>, press or release")),
                    }
                }
            };
            Ok((t, step))
        })
        .collect()
}

pub struct WaitPlugin;

impl Plugin for WaitPlugin {
    fn build(&self, app: &mut App) {
        let aid = match std::env::var("NVF_PLAY_TEST_WAIT").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_WAIT: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<WaitMenu>()
            .init_resource::<SleepFade>()
            .add_systems(Update, fade.after(PlaySet::Ui))
            .init_resource::<WaitStats>()
            .insert_resource(WaitAid(aid))
            .add_message::<OpenWait>()
            .add_message::<WaitInput>()
            .add_message::<HourPassed>()
            .add_message::<RestEnded>()
            .add_systems(Startup, read_words)
            .add_systems(PreUpdate, aid_pointer.after(bevy::input::InputSystems).before(bevy::ui::UiSystems::Focus))
            // Before the scripts, as the running clock is (`crate::sim`): a frame's
            // scripts all see the hour that passed in it.
            .add_systems(Update, pass_hours.run_if(in_state(PlayState::Menu)).before(PlaySet::Scripts))
            .add_systems(Update, pass_behind_screen.run_if(in_state(PlayState::Loading)).run_if(|m: Res<WaitMenu>| m.is_open()))
            .add_systems(
                Update,
                (
                    rest_opens.run_if(in_state(PlayState::Playing)),
                    run_aid,
                    script_requests,
                    open,
                    (keys, mouse, apply, draw, set_glyphs).chain().run_if(in_state(PlayState::Menu)),
                )
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
    }
}

/// The menu's wording, from the game settings of the plugins in use.
fn read_words(sim: Res<SimState>, mut commands: Commands) {
    let words = Words::from_settings(|name| match sim.forms.settings.get(&name.to_ascii_lowercase()) {
        Some(fnv_sim::settings::Setting::Str(s)) if !s.trim().is_empty() => Some(s.clone()),
        _ => None,
    });
    commands.insert_resource(words);
    let refusals = RefusalWords::from_settings(|name| match sim.forms.settings.get(&name.to_ascii_lowercase()) {
        Some(fnv_sim::settings::Setting::Str(s)) if !s.trim().is_empty() => Some(s.clone()),
        _ => None,
    });
    commands.insert_resource(refusals);
}

fn rest_opens(input: ControlInput, mut out: MessageWriter<OpenWait>, time: Res<Time<Real>>, tweaks: Option<Res<crate::tweaks::TweakSettings>>, mut held: Local<Option<f32>>) {
    // Quick Loot has the key while its list shows: Take All (`crate::quick_loot`).
    if crate::quick_loot::holds_input() {
        *held = None;
        return;
    }
    // The player's tweak: the key is held a while, not tapped [design: docs/design/tweaks.md].
    if tweaks.is_some_and(|t| t.hold_to_wait) {
        if input.pressed(Control::Rest) {
            let t = held.get_or_insert(0.0);
            let before = *t;
            *t += time.delta_secs();
            if before < REST_HOLD && *t >= REST_HOLD {
                out.write(OpenWait::default());
            }
        } else {
            *held = None;
        }
        return;
    }
    if input.just_pressed(Control::Rest) {
        out.write(OpenWait::default());
    }
}

/// How long the Hold to Wait tweak wants the key held, seconds [design].
const REST_HOLD: f32 = 0.5;

/// What scripts asked of the menu since the last frame (`fnv_game::rest`).
fn script_requests(mut script: Option<ResMut<ScriptState>>, mut menu: ResMut<WaitMenu>, mut open: MessageWriter<OpenWait>, mut input: MessageWriter<WaitInput>) {
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    for r in rt.game.rest.take_requests() {
        info!("wait menu: a script asks: {r:?}");
        match (r, menu.0.as_mut()) {
            (RestRequest::Menu { sleep, check }, _) => {
                open.write(OpenWait { sleep: Some(sleep), unchecked: !check, hours: None });
            }
            // With the menu open the hours start in it; a sleep already passing takes
            // the new count.
            (RestRequest::Sleep { hours }, Some(m)) => {
                m.sleep_for(hours);
                match m.passing {
                    Some(p) => rt.game.begin_rest(true, p.asked),
                    None => rt.game.rest.end(),
                }
            }
            (RestRequest::Sleep { hours }, _) => {
                open.write(OpenWait { sleep: Some(true), unchecked: true, hours: Some(hours) });
            }
            (RestRequest::Wake, Some(m)) if m.sleep => {
                input.write(WaitInput(Command::Cancel));
            }
            (RestRequest::Wake, _) => {}
        }
    }
}

/// Why the player cannot wait or sleep now, if anything forbids it (wait-menu.md §9).
/// The original's refusals are known by the names of their texts only; the order they
/// are tried in here is NVForever's [engineering choice].
#[allow(clippy::too_many_arguments)]
fn refusal(
    sleep: bool,
    bed: Option<u32>,
    rt: Option<&mut crate::script::Runtime>,
    sim: &mut SimState,
    swimming: bool,
    airborne: bool,
    feet: Option<[f32; 3]>,
    indoors: bool,
    actors: &[(u32, [f32; 3])],
    distance: f32,
    seated: bool,
) -> Option<(Refusal, String)> {
    // The player's tweak: waiting only while sitting (god mode excepted) [design:
    // docs/design/tweaks.md].
    if !sleep && !seated && !crate::combat::god_mode() && rt.as_ref().is_some_and(|rt| rt.game.tweaks.wait_only_seated) {
        return Some((Refusal::NotSeated, "the player does not sit (Wait Only Seated)".into()));
    }
    // A script forbade waiting (`EnableFastTravel`'s canWait): the same flag refuses
    // sleeping, and its text is the "cannot wait in this cell" one [documented: GECK wiki
    // SetPCHasSleepWaitOverride, GetPCHasSleepWaitOverride].
    if rt.as_ref().is_some_and(|rt| rt.game.player.fast_travel.wait_disabled) {
        return Some((Refusal::Forbidden, "a script has turned waiting off (EnableFastTravel)".into()));
    }
    // The player's tweak: nothing else refuses [design: docs/design/tweaks.md].
    if rt.as_ref().is_some_and(|rt| rt.game.tweaks.wait_anywhere) {
        return None;
    }
    // [hypothesis: "under water" is the swimming state; the text's name speaks of
    // waiting only, and nobody lies on a bed swimming.]
    if swimming {
        return Some((Refusal::UnderWater, "the player is swimming".into()));
    }
    // In the air: jumping or falling [owner request 2026-10-17].
    if airborne {
        return Some((Refusal::Falling, "the player is in the air".into()));
    }
    // Taking radiation damage [owner request 2026-10-17]: rads from the world now
    // (markers, water, explosion zones: docs/spec-notes/radiation.md).
    if taking_radiation_damage(rt.as_deref()) {
        return Some((Refusal::Radiation, "the player is taking radiation damage".into()));
    }
    let rt = rt?;
    crate::script::with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        if let Some(feet) = feet {
            let near = |p: &[f32; 3]| (0..3).map(|k| (p[k] - feet[k]).powi(2)).sum::<f32>() <= distance * distance;
            let close: Vec<FormId> = actors.iter().filter(|(_, p)| near(p)).map(|(id, _)| FormId(*id)).collect();
            let hostile = close.iter().copied().find(|&a| fnv_game::rest::hostile_to_player(g, a));
            info!("wait menu: {} living actors drawn, {} within {distance:.0} units, {}", actors.len(), close.len(), if hostile.is_some() { "one of them hostile" } else { "none hostile" });
            if let Some(a) = hostile {
                let place = if indoors { "indoors" } else { "outdoors" };
                return Some((Refusal::HostilesNear, format!("{} is hostile and within {distance:.0} units ({place})", g.data.name(a))));
            }
        }
        // In combat: a drawn actor in a combat state has the player on its target or
        // alert list, as IsInCombat on the player reads it (`fnv_game::awareness`) [owner
        // request 2026-10-17; the texts' names are documented, wait-menu.md §9]. After the
        // hostiles near, whose line is the original's own [hypothesis: in a fight the
        // enemy is near]; this catches a hunter farther away, or one the player provoked
        // that would not attack on its own. Only the drawn ones count: an actor left
        // behind in an unloaded cell keeps its combat state, and the player who went away
        // is no longer fought [hypothesis].
        let player = FormId::PLAYER_REF;
        let hunter = actors.iter().map(|(id, _)| FormId(*id)).find(|&a| {
            g.awareness.get(a).is_some_and(|w| w.state.in_combat() && (w.targets.contains_key(&player) || w.alarms.iter().any(|x| x.owner == player)))
        });
        if let Some(a) = hunter {
            return Some((Refusal::InCombat, format!("{} is in a combat state and hunts the player", g.data.name(a))));
        }
        // A cell flagged "Can't wait" (CELL header bit 19, docs/spec-notes/cells-refs.md §1)
        // [hypothesis: the line is the one a script's ban gives, "You cannot wait here"].
        if let Some(cell) = g.cant_wait_here() {
            return Some((Refusal::Forbidden, format!("{} is flagged Can't Wait", g.data.name(cell))));
        }
        // Trespassing: an owned cell, not public, that is not the player's, the cell
        // `fnv_game::crime` warns the player in [owner request 2026-10-17].
        if let Some((cell, _)) = g.trespass_cell() {
            return Some((Refusal::Trespassing, format!("the player trespasses in {}", g.data.name(cell))));
        }
        // The player's tweak lets them sleep in it [design: docs/design/tweaks.md].
        let owner = bed.filter(|_| sleep && !g.tweaks.sleep_in_owned_beds).and_then(|b| fnv_game::rest::bed_owned_by_another(g, FormId(b)))?;
        Some((Refusal::OwnedBed, format!("the bed belongs to {}", g.data.name(owner))))
    })
}

/// Whether the player takes radiation damage now: rads from the world (radiation
/// markers, irradiated water, explosions' zones) after Rad Resistance [owner request
/// 2026-10-17; docs/spec-notes/radiation.md §4].
fn taking_radiation_damage(rt: Option<&crate::script::Runtime>) -> bool {
    rt.is_some_and(|rt| rt.game.radiation().rate > 0.0)
}

/// Whether the player is in the world, where a request opens the menu: no menu, no
/// conversation, no loading screen and no script's menu or box is up.
fn in_world(state: &PlayState, menu: &WaitMenu, active: &ActiveMenu, paused: &Paused, script: Option<&ScriptState>) -> bool {
    let in_script_menu = script.is_some_and(|s| s.rt.as_ref().is_some_and(|rt| rt.game.in_menu()));
    *state == PlayState::Playing && !menu.is_open() && active.0.is_none() && !paused.0 && !in_script_menu
}

/// The aid's steps that are due. An `open` waits until the player is in the world (the
/// start-up message boxes come a few seconds after the scripts are ready, and that is
/// later on a loaded machine), the steps after it held as long [decision]. With the menu
/// itself open already (lying down on a bed opens it) an `open` is not held: it is a
/// request the menu ignores, and the steps after it are for that menu.
#[allow(clippy::too_many_arguments)]
fn run_aid(
    time: AidTime,
    mut aid: ResMut<WaitAid>,
    mut held_since: Local<Option<f32>>,
    state: Res<State<PlayState>>,
    menu: Res<WaitMenu>,
    (active, paused, script): (Res<ActiveMenu>, Res<Paused>, Option<Res<ScriptState>>),
    mut open: MessageWriter<OpenWait>,
    mut input: MessageWriter<WaitInput>,
) {
    if aid.0.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let held = match menu.is_open() || in_world(state.get(), &menu, &active, &paused, script.as_deref()) {
        true => None,
        false => hold_open(&mut aid, now),
    };
    let due = match held {
        Some((before, by)) => {
            if held_since.is_none() {
                info!("test aid NVF_PLAY_TEST_WAIT: open held, the player is not in the world ({by:.2} s late)");
                *held_since = Some(now - by);
            }
            before
        }
        None => {
            if let Some(t) = held_since.take() {
                info!("test aid NVF_PLAY_TEST_WAIT: open {:.2} s later than asked; the steps after it as much later", now - t);
            }
            due_steps(&mut aid, now, false)
        }
    };
    for step in due {
        info!("test aid NVF_PLAY_TEST_WAIT: {step:?}");
        match step {
            AidStep::Open => {
                open.write(OpenWait::default());
            }
            AidStep::Input(c) => {
                input.write(WaitInput(c));
            }
            _ => {}
        }
    }
}

/// The aid's pointer: where the window has its cursor and what its left button does,
/// after the real input was read and before the UI looks for what is under the cursor.
fn aid_pointer(
    time: AidTime,
    mut aid: ResMut<WaitAid>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    if aid.0.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    for step in due_steps(&mut aid, now, true) {
        info!("test aid NVF_PLAY_TEST_WAIT: {step:?}");
        match step {
            AidStep::Point(x, y) => {
                if let Ok(mut w) = windows.single_mut() {
                    let at = Vec2::new(x * w.width(), y * w.height());
                    w.set_cursor_position(Some(at));
                }
            }
            AidStep::Press => buttons.press(MouseButton::Left),
            AidStep::Release => buttons.release(MouseButton::Left),
            _ => {}
        }
    }
}

/// Open the menu on request while playing: as a Sleep menu when the player lies on a
/// bed. It stays shut while a script has turned the Pip-Boy off [documented: GECK wiki
/// DisablePlayerControls, "the Pip-Boy cannot be brought up, nor the Wait menu";
/// controls.md §5].
#[allow(clippy::too_many_arguments)]
fn open(
    mut requests: MessageReader<OpenWait>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut menu: ResMut<WaitMenu>,
    active: Res<ActiveMenu>,
    paused: Res<Paused>,
    seat: Res<PlayerSeat>,
    mut sim: ResMut<SimState>,
    (words, refusals): (Res<Words>, Res<RefusalWords>),
    mut stats: ResMut<WaitStats>,
    mut script: Option<ResMut<ScriptState>>,
    locks: Option<Res<ControlLocks>>,
    lock_stats: Option<ResMut<LockStats>>,
    (phys, location, actors, mut corner): (Res<crate::phys::PhysState>, Res<crate::Location>, Res<crate::hitvol::HitVolumes>, Option<ResMut<crate::corner_messages::CornerMessages>>),
) {
    // One menu a frame: the last request stands.
    let Some(asked) = requests.read().last().copied() else { return };
    let by_script = asked.sleep.is_some();
    if !by_script && locks.is_some_and(|l| l.pipboy) {
        info!("wait menu: stays shut, a script has disabled the player's controls (Pip-Boy)");
        stats.refused += 1;
        if let Some(mut s) = lock_stats {
            s.held[group::PIPBOY] += 1;
        }
        return;
    }
    if !in_world(state.get(), &menu, &active, &paused, script.as_deref()) {
        info!("wait menu: request ignored, the player is not in the world (a menu, a conversation or a loading screen is up)");
        return;
    }
    // The player's tweak: waiting on a chair is sleeping [design: docs/design/tweaks.md].
    let chair_sleep = seat.on.is_some() && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.sleep_on_chairs);
    let sleep = asked.sleep.unwrap_or(seat.bed || chair_sleep);
    let bed = seat.on.filter(|_| seat.bed);
    if !asked.unchecked {
        let player = phys.player.as_ref();
        let indoors = location.interior.is_some();
        let distance = ["fHostileActorExteriorDistance", "fHostileActorInteriorDistance"].map(|name| match sim.forms.settings.get(&name.to_ascii_lowercase()) {
            Some(fnv_sim::settings::Setting::Float(v)) => Some(*v),
            _ => None,
        })[usize::from(indoors)]
        .unwrap_or(HOSTILE_DISTANCE[usize::from(indoors)]);
        let rt = script.as_mut().and_then(|s| s.rt.as_mut());
        // Furniture holds the player off the ground's rules (a bed, a chair).
        let airborne = seat.on.is_none() && player.is_some_and(|c| !c.grounded && !c.swimming);
        let no = refusal(sleep, bed, rt, &mut sim, player.is_some_and(|c| c.swimming), airborne, player.map(|c| c.feet), indoors, &actors.living, distance, seat.on.is_some());
        if let Some((why, detail)) = no {
            let line = refusals.line(why, sleep);
            info!("wait menu: refused ({}): {detail}; on screen: {line:?}", if sleep { "sleep" } else { "wait" });
            // A top-left corner message (owner, list 5 item 44).
            if let Some(c) = corner.as_deref_mut() {
                c.push(line.into());
            }
            stats.turned_down[why as usize] += 1;
            return;
        }
    }
    let mut m = Model::open(sleep);
    stats.opened += 1;
    stats.as_sleep += u32::from(m.sleep);
    match sim.world.calendar() {
        Ok(c) => info!(
            "wait menu: open as {} at {} (GameHour {:.3}, GameDaysPassed {:.3})",
            if m.sleep { "sleep (the player lies on a bed)" } else { "wait" },
            words.date_line(&c),
            c.hour,
            c.days_passed
        ),
        Err(e) => warn!("wait menu: no calendar: {e}"),
    }
    // Scripts run MenuMode 1012 from now on.
    if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
        // GetSleepWaitMenuState tells the sleep from the wait (`fnv_game::nvse_jg8`).
        rt.game.other.jg8.sleep_menu = m.sleep;
        rt.game.open_menu(crate::script::MENU_SLEEP_WAIT);
        // The bed's own script learns that the player sleeps on it.
        if let Some(bed) = bed.filter(|_| sleep) {
            if crate::script::with_world(rt, &mut sim, |rt| rt.bed_activated_for_sleep(FormId(bed))) {
                info!("wait menu: the bed {bed:08X} has a script: its OnActivate block ran for this sleep");
                stats.beds_told += 1;
            }
        }
        if let Some(hours) = asked.hours {
            m.sleep_for(hours);
            if let Some(p) = m.passing {
                info!("wait menu: sleeping {} hour(s) (a script's SetPCSleepHours)", p.asked);
                rt.game.begin_rest(m.sleep, p.asked);
            }
        }
    }
    menu.0 = Some(m);
    next.set(PlayState::Menu);
}

fn keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    input: ControlInput,
    menu: Res<WaitMenu>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut out: MessageWriter<WaitInput>,
) {
    let keys = keys.snapshot();
    use KeyCode::*;
    if !menu.is_open() {
        return;
    }
    let wasd = tweaks.as_ref().is_some_and(|t| t.game.wasd_as_arrow_keys);
    let mut send = |c| {
        out.write(WaitInput(c));
    };
    if keys.any_just_pressed([KeyX, Escape]) || input.just_pressed(Control::Rest) {
        send(Command::Cancel);
        return;
    }
    let accept: &[KeyCode] = if wasd { &[KeyE, Enter, NumpadEnter, Space] } else { &[KeyE, Enter, NumpadEnter] };
    if keys.any_just_pressed(accept.iter().copied()) {
        send(Command::Accept);
    }
    if keys.any_just_pressed([ArrowLeft, KeyA]) {
        send(Command::Step(-1));
    }
    if keys.any_just_pressed([ArrowRight, KeyD]) {
        send(Command::Step(1));
    }
    if keys.just_pressed(PageDown) {
        send(Command::Step(-JUMP));
    }
    if keys.just_pressed(PageUp) {
        send(Command::Step(JUMP));
    }
    if keys.just_pressed(Home) {
        send(Command::Hours(1));
    }
    if keys.just_pressed(End) {
        send(Command::Hours(MAX_HOURS));
    }
}

fn mouse(
    menu: Res<WaitMenu>,
    track: Query<(&PointerState, &RelativeCursorPosition), With<SliderTrack>>,
    buttons: Query<(&PointerState, &WaitButton), Changed<PointerState>>,
    mut wheel: MessageReader<MouseWheel>,
    mut lines: Local<f32>,
    mut out: MessageWriter<WaitInput>,
) {
    let Some(m) = menu.0.as_ref() else { return };
    // A press on the slider, and the drag after it, put the marker under the pointer.
    for (i, at) in &track {
        if let (PointerState::Pressed, Some(p)) = (i, at.normalized) {
            let hours = model::hours_at(p.x + 0.5);
            if hours != m.hours {
                out.write(WaitInput(Command::Hours(hours)));
            }
        }
    }
    for (i, b) in &buttons {
        if *i == PointerState::Pressed {
            out.write(WaitInput(b.0));
        }
    }
    for w in wheel.read() {
        *lines += match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 20.0,
        };
    }
    // Wheel up moves the marker right [documented: the interface file's slider].
    let whole = lines.trunc();
    if whole != 0.0 {
        out.write(WaitInput(Command::Step(whole as i32)));
        *lines -= whole;
    }
}

fn apply(mut input: MessageReader<WaitInput>, mut menu: ResMut<WaitMenu>, mut next: ResMut<NextState<PlayState>>, mut script: Option<ResMut<ScriptState>>) {
    for &WaitInput(cmd) in input.read() {
        let Some(m) = menu.0.as_mut() else { return };
        let before = m.clone();
        let close = m.apply(cmd);
        match (cmd, &m.passing, close) {
            (Command::Accept, Some(p), _) if before.passing.is_none() => {
                info!("wait menu: {} {} hour(s)", if m.sleep { "sleeping" } else { "waiting" }, p.asked);
                // To scripts the player sleeps (or waits) from now until the menu closes.
                if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                    rt.game.begin_rest(m.sleep, p.asked);
                }
            }
            (Command::Cancel, _, true) => match &before.passing {
                Some(p) => info!("wait menu: stopped after {} of {} hours", p.asked - p.left, p.asked),
                None => info!("wait menu: cancelled"),
            },
            _ => {}
        }
        if close {
            next.set(PlayState::Playing);
            return;
        }
    }
}

/// While the accepted hours pass: one hour onto the game's calendar every
/// [`HOUR_SECONDS`], a log line for each, and the menu closes after the last. The time
/// is the aids' clock, which goes on while a menu holds the picture (`crate::pause`).
///
/// Each hour also passes for the player's body (`fnv_game::Game::pass_hours`): active
/// effects advance by the seconds of play an hour stands for, Hardcore's needs rise, a
/// sleep lowers sleep deprivation (docs/spec-notes/aid-items.md §6, §11); a line says
/// what changed.
#[allow(clippy::too_many_arguments)]
fn pass_hours(
    time: AidTime,
    mut menu: ResMut<WaitMenu>,
    mut sim: ResMut<SimState>,
    words: Res<Words>,
    mut stats: ResMut<WaitStats>,
    mut next: ResMut<NextState<PlayState>>,
    mut passed: MessageWriter<HourPassed>,
    mut script: Option<ResMut<ScriptState>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let Some(m) = menu.0.as_mut() else { return };
    let pace = hour_seconds(tweaks.as_ref().is_some_and(|t| t.game.faster_sleep_wait));
    if m.passing.is_none() || !m.tick(time.delta_secs_f64() as f32, pace) {
        return;
    }
    let p = m.passing.expect("an hour passed");
    let mut hours = Hours { sim: &mut sim, words: &words, stats: &mut stats, passed: &mut passed, script: script.as_deref_mut() };
    if !hours.pass(m.sleep, p.asked - p.left, p.asked) {
        next.set(PlayState::Playing);
        return;
    }
    if p.left == 0 {
        info!("wait menu: {} hour(s) passed; closed", p.asked);
        next.set(PlayState::Playing);
    }
}

/// What one hour of a wait or a sleep moves on: the calendar, the world's people (an
/// [`HourPassed`]), the player's body and the scripts (`crate::aid::hour_passed`).
struct Hours<'a, 'w> {
    sim: &'a mut SimState,
    words: &'a Words,
    stats: &'a mut WaitStats,
    passed: &'a mut MessageWriter<'w, HourPassed>,
    script: Option<&'a mut ScriptState>,
}

impl Hours<'_, '_> {
    /// Hour `hour` of `asked` passes; false when the game clock cannot advance.
    fn pass(&mut self, sleep: bool, hour: u32, asked: u32) -> bool {
        match self.sim.world.advance_hours(1.0) {
            Ok(c) => {
                self.stats.hours += 1;
                self.passed.write(HourPassed { sleep });
                if let Some(rt) = self.script.as_mut().and_then(|s| s.rt.as_mut()) {
                    crate::script::with_world(rt, self.sim, |rt| crate::aid::hour_passed(rt, sleep, hour));
                }
                info!(
                    "{}: hour {hour} of {asked} passed: {} (GameHour {:.3}, GameDaysPassed {:.3})",
                    if sleep { "sleep" } else { "wait" },
                    self.words.date_line(&c),
                    c.hour,
                    c.days_passed
                );
                true
            }
            Err(e) => {
                warn!("wait menu: the game clock cannot advance: {e}");
                false
            }
        }
    }
}

/// A trip's loading screen is up with the menu still open: a script's sleep asked in the
/// frame the trip began (the raft between Cottonwood Cove and the Fort: `MoveTo`, then
/// `SetPCSleepHours 3`, issue #2281), or a trip that began while the menu was open. The
/// hours still to pass go by at once behind the screen, and the menu closes without
/// being seen, so the world comes back with the time moved on and nothing left to wait
/// for [design: the screen already hides the world, so the hours take no time of the
/// player's own]. Without this the loading screen hands the game back to play with the
/// menu still open, where its hours never pass and its fade keeps the screen black.
#[allow(clippy::too_many_arguments)]
fn pass_behind_screen(
    mut commands: Commands,
    mut menu: ResMut<WaitMenu>,
    mut sim: ResMut<SimState>,
    words: Res<Words>,
    mut stats: ResMut<WaitStats>,
    mut passed: MessageWriter<HourPassed>,
    mut ended: MessageWriter<RestEnded>,
    mut script: Option<ResMut<ScriptState>>,
    mut fade: ResMut<SleepFade>,
    roots: Query<Entity, With<WaitRoot>>,
) {
    let Some(m) = menu.0.as_mut() else { return };
    if let Some(left) = m.take_left() {
        let (sleep, asked) = (m.sleep, *left.end());
        info!("wait menu: a trip's loading screen is up: hours {}-{asked} of the {} pass behind it at once", left.start(), if sleep { "sleep" } else { "wait" });
        let mut hours = Hours { sim: &mut sim, words: &words, stats: &mut stats, passed: &mut passed, script: script.as_deref_mut() };
        for hour in left {
            if !hours.pass(sleep, hour, asked) {
                break;
            }
        }
    } else {
        info!("wait menu: closed under a trip's loading screen");
    }
    // The screen hides the world: no fade to come back from.
    fade.0 = 0.0;
    close_menu(&mut commands, &mut menu, &roots, script.as_deref_mut(), &mut ended);
}

fn fade(time: AidTime, menu: Res<WaitMenu>, mut fade: ResMut<SleepFade>) {
    let sleeping = menu.0.as_ref().is_some_and(|m| m.sleep && m.passing.is_some());
    let next = fade_step(fade.0, sleeping, time.delta_secs_f64() as f32);
    if next != fade.0 {
        fade.0 = next;
    }
}

/// The menu state is left. When it is left for a trip's loading screen (the state is
/// already the new one here) the menu stays for [`pass_behind_screen`], which passes its
/// hours first.
fn close(
    mut commands: Commands,
    state: Res<State<PlayState>>,
    mut menu: ResMut<WaitMenu>,
    roots: Query<Entity, With<WaitRoot>>,
    mut script: Option<ResMut<ScriptState>>,
    mut ended: MessageWriter<RestEnded>,
) {
    if *state.get() == PlayState::Loading && menu.is_open() {
        return;
    }
    close_menu(&mut commands, &mut menu, &roots, script.as_deref_mut(), &mut ended);
}

fn close_menu(commands: &mut Commands, menu: &mut WaitMenu, roots: &Query<Entity, With<WaitRoot>>, script: Option<&mut ScriptState>, ended: &mut MessageWriter<RestEnded>) {
    if let Some(m) = menu.0.take() {
        if let Some(p) = m.passing.filter(|p| p.asked > p.left) {
            ended.write(RestEnded { sleep: m.sleep, hours: p.asked - p.left });
        }
        if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
            if rt.game.menu == Some(crate::script::MENU_SLEEP_WAIT) {
                rt.game.close_menu();
            }
            rt.game.rest.end();
        }
    }
    for e in roots {
        commands.entity(e).despawn();
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<WaitStats>) {
    let turned_down: u32 = stats.turned_down.iter().sum();
    if exit.read().next().is_none() || (stats.opened == 0 && stats.refused == 0 && turned_down == 0) {
        return;
    }
    let why: Vec<String> = Refusal::ALL.iter().filter(|r| stats.turned_down[**r as usize] > 0).map(|r| format!("{} {}", stats.turned_down[*r as usize], r.label())).collect();
    info!(
        "wait summary: the menu was opened {} times ({} as sleep), {} hours passed, {} requests held by scripts, {} refused{}, {} sleeps told to a bed's script",
        stats.opened,
        stats.as_sleep,
        stats.hours,
        stats.refused,
        turned_down,
        if why.is_empty() { String::new() } else { format!(" ({})", why.join(", ")) },
        stats.beds_told
    );
}

// ---- drawing -----------------------------------------------------------------------
//
// The layout is measured on the owner's 1280 x 720 screenshot of the original menu
// [observed; docs/spec-notes/wait-menu.md §2] and drawn in parts of the view's height,
// so it keeps its place and size on any window. Its colour is the HUD's and its texts are
// in the game's fonts [documented: the original's `menus\sleep_wait_menu.xml`: system
// colour `&hudmain;`, the question in font 1, the hours in font 6, the time in font 2;
// the buttons' font is the text box prefab's, taken as font 2 (hypothesis); owner, todo 5
// item 51: the menu looked faded beside the HUD].

#[derive(Component)]
struct WaitRoot;

#[derive(Component)]
struct TitleText;

#[derive(Component)]
struct HoursText;

#[derive(Component)]
struct DateText;

/// The triangle over the slider.
#[derive(Component)]
struct Marker;

/// The slider's clickable length: from its first end mark to its last.
#[derive(Component)]
struct SliderTrack;

#[derive(Component, Clone, Copy)]
struct WaitButton(Command);

/// The HUD's colour (`crate::interface::AMBER`) [documented: `&hudmain;`]. The first
/// colour, the slider's line in the screenshot (179, 140, 65), was the HUD colour dimmed
/// by the screenshot's own brightness [observed: the HP bar of the owner's screenshots is
/// (255, 182, 66)].
const AMBER: Color = Color::srgb(crate::interface::AMBER[0], crate::interface::AMBER[1], crate::interface::AMBER[2]);
/// The fonts by the number the menu file gives them [documented: Fallout_default.ini].
const TITLE_FONT: usize = 1;
const HOURS_FONT: usize = 6;
const TIME_FONT: usize = 2;
const BUTTON_FONT: usize = 2;
/// How opaque a text is: the three lines and the button under the pointer; a button the
/// pointer is not on [documented: the interface file's buttons are at half opacity
/// unless targeted; observed: "Wait E)" at 0.59 of "Cancel X)" in the screenshot]; the
/// accept button while the hours pass [design]. Bevy mixes in linear light, where 0.3
/// reads as that 0.59.
const BRIGHT: f32 = 1.0;
const NORMAL: f32 = 0.3;
const FAINT: f32 = 0.12;
/// The box darkens what is behind it [observed: the sky inside the box is at a quarter
/// of its brightness outside, which in linear light is a twentieth].
const SHADE: Color = Color::srgba(0.0, 0.0, 0.0, 0.93);

/// The box, in the screenshot's pixels.
const BOX: Vec2 = Vec2::new(643.0, 300.0);
/// The slider: its marks start here and are this far apart; marks 1 to 24 are the hours,
/// 0 and 25 the end marks.
const MARK_X0: f32 = 80.0;
const MARK_STEP: f32 = 19.4;
/// The top of the slider's line.
const LINE_Y: f32 = 140.0;
/// The marker over the slider: its width and height; its point ends this far above the
/// line.
const MARKER: Vec2 = Vec2::new(27.0, 13.5);
const MARKER_GAP: f32 = 17.0;

/// A button's width: room for its label, which ends at its right [design].
const BUTTON_WIDTH: f32 = 200.0;

/// A length of the screenshot (720 pixels high) as a part of the view's height, at the UI
/// scale (`crate::interface::unit_scale`, #1631; at 100% `px / 7.2` exactly).
fn u(px: f32) -> Val {
    Val::Vh(px * crate::interface::unit_scale() / 7.2)
}

fn shade(part: f32) -> Color {
    AMBER.with_alpha(part)
}

/// The menu's fonts, read when it is first drawn.
#[derive(Resource)]
struct WaitFaces(Vec<(usize, crate::interface::Face)>);

impl WaitFaces {
    fn get(&self, n: usize) -> Option<&crate::interface::Face> {
        self.0.iter().find(|(k, _)| *k == n).map(|(_, f)| f)
    }
}

/// A text drawn in a game font: what it says and its colour now, and what was drawn.
#[derive(Component)]
struct Glyphs {
    font: usize,
    text: String,
    color: Color,
    /// Right-aligned (the buttons) or centred.
    right: bool,
    drawn: Option<(String, Color)>,
}

impl Glyphs {
    fn new(font: usize, right: bool) -> Glyphs {
        Glyphs { font, text: String::new(), color: shade(BRIGHT), right, drawn: None }
    }
}

fn at(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: u(left), top: u(top), width: u(width), height: u(height), ..default() }
}

/// A line of text across the box, centred on `y`.
fn line(y: f32) -> Node {
    Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..at(0.0, y - 15.0, BOX.x, 30.0) }
}

fn spawn_menu(commands: &mut Commands) {
    let screen = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    commands.spawn((WaitRoot, screen, GlobalZIndex(105))).with_children(|root| {
        root.spawn((Node { width: u(BOX.x), height: u(BOX.y), ..default() }, BackgroundColor(SHADE))).with_children(|b| {
            // The outline: a line along the top and the bottom, and at each corner a
            // short side that fades out.
            for y in [0.0, BOX.y - 1.5] {
                b.spawn((at(0.0, y, BOX.x, 1.5), BackgroundColor(AMBER)));
            }
            for x in [0.0, BOX.x - 1.5] {
                for (i, part) in [0.85, 0.55, 0.25].into_iter().enumerate() {
                    let step = 10.0 * i as f32;
                    b.spawn((at(x, step, 1.5, 10.0), BackgroundColor(shade(part))));
                    b.spawn((at(x, BOX.y - 10.0 - step, 1.5, 10.0), BackgroundColor(shade(part))));
                }
            }
            b.spawn((TitleText, Glyphs::new(TITLE_FONT, false), line(61.0)));
            // The slider: the line, the hour marks on it and the two end marks.
            b.spawn((at(65.0, LINE_Y, 516.0, 3.0), BackgroundColor(AMBER)));
            for k in 1..=MAX_HOURS {
                b.spawn((at(MARK_X0 + MARK_STEP * k as f32 - 1.0, LINE_Y - 6.0, 2.0, 6.0), BackgroundColor(AMBER)));
            }
            let last = MARK_X0 + MARK_STEP * (MAX_HOURS + 1) as f32;
            for (x, foot) in [(MARK_X0, -7.0), (last, 2.5)] {
                b.spawn((at(x - 1.25, LINE_Y - 12.0, 2.5, 12.0), BackgroundColor(AMBER)));
                b.spawn((at(x + foot - 1.25, LINE_Y - 4.0, 7.0, 4.0), BackgroundColor(AMBER)));
            }
            // The marker: the lower half of a square standing on its corner.
            let side = MARKER.x / std::f32::consts::SQRT_2;
            b.spawn((Marker, Node { overflow: Overflow::clip(), ..at(0.0, LINE_Y - MARKER_GAP - MARKER.y, MARKER.x, MARKER.y) })).with_children(|m| {
                m.spawn((
                    at((MARKER.x - side) / 2.0, -side / 2.0, side, side),
                    BackgroundColor(AMBER),
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                ));
            });
            // What takes the mouse: the slider's length, from above the marker to under
            // the line.
            b.spawn((SliderTrack, Button, RelativeCursorPosition::default(), at(MARK_X0, LINE_Y - 34.0, MARK_STEP * (MAX_HOURS + 1) as f32, 46.0)));
            b.spawn((HoursText, Glyphs::new(HOURS_FONT, false), line(168.0)));
            b.spawn((DateText, Glyphs::new(TIME_FONT, false), line(243.0)));
            // The two buttons, their right ends a little outside the box's.
            for (y, cmd) in [(205.0, Command::Accept), (243.0, Command::Cancel)] {
                let node = Node { position_type: PositionType::Absolute, right: u(-5.0), top: u(y - 15.0), width: u(BUTTON_WIDTH), height: u(30.0), ..default() };
                b.spawn((Button, WaitButton(cmd), node, Glyphs { color: shade(NORMAL), ..Glyphs::new(BUTTON_FONT, true) }));
            }
        });
    });
}

/// Draw the menu when it opens; after that only what changes: the texts, the marker's
/// place, the buttons' shade.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    menu: Res<WaitMenu>,
    sim: Res<SimState>,
    words: Res<Words>,
    roots: Query<Entity, With<WaitRoot>>,
    mut texts: ParamSet<(
        Query<&mut Glyphs, With<TitleText>>,
        Query<&mut Glyphs, With<HoursText>>,
        Query<&mut Glyphs, With<DateText>>,
        Query<(&mut Glyphs, &WaitButton, &PointerState)>,
    )>,
    mut marker: Query<&mut Node, With<Marker>>,
    (tiles, script): (Option<Res<crate::tiles::vanilla::TileMenus>>, Option<Res<ScriptState>>),
) {
    let Some(m) = menu.0.as_ref() else { return };
    // T15b: with `[UI] TileMenus=SleepWaitMenu` the menu is drawn from its file
    // (`crate::tiles::wait_menu`); the hand-built one is not spawned, or goes.
    let tiled = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| crate::tiles::wait_menu::tiled(tiles.as_ref().is_some_and(|t| t.sleep_wait), &rt.game));
    if tiled {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if roots.is_empty() {
        spawn_menu(&mut commands);
        // Filled in from the next frame on, when the texts exist.
        return;
    }
    let Ok(cal) = sim.world.calendar() else { return };
    let view = m.view(&words, &cal);
    let set = |g: &mut Glyphs, s: &str| {
        if g.text != s {
            g.text = s.to_string();
        }
    };
    for mut g in &mut texts.p0() {
        set(&mut g, &view.title);
    }
    for mut g in &mut texts.p1() {
        set(&mut g, &view.hours);
    }
    for mut g in &mut texts.p2() {
        set(&mut g, &view.date);
    }
    for mut node in &mut marker {
        let left = u(MARK_X0 + MARK_STEP * view.marker as f32 - MARKER.x / 2.0);
        if node.left != left {
            node.left = left;
        }
    }
    for (mut g, b, interaction) in &mut texts.p3() {
        let (label, live) = match b.0 {
            Command::Accept => (&view.accept, view.can_accept),
            _ => (&view.cancel, true),
        };
        set(&mut g, label);
        let part = match (live, interaction) {
            (false, _) => FAINT,
            (true, PointerState::None) => NORMAL,
            (true, _) => BRIGHT,
        };
        if g.color != shade(part) {
            g.color = shade(part);
        }
    }
}

/// Set the texts that changed again in their fonts: centred in their line, or ending at
/// its right.
fn set_glyphs(
    mut commands: Commands,
    mut texts: Query<(Entity, &mut Glyphs, &Node)>,
    faces: Option<Res<WaitFaces>>,
    data: Option<Res<nvf_assets::GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<bevy::image::CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
) {
    if texts.is_empty() {
        return;
    }
    let Some(faces) = faces else {
        let bc = crate::interface::bc_supported(formats.as_deref());
        let mut loaded: Vec<(usize, crate::interface::Face)> = Vec::new();
        for n in [TITLE_FONT, HOURS_FONT, TIME_FONT, BUTTON_FONT] {
            if loaded.iter().any(|(k, _)| *k == n) {
                continue;
            }
            let path = crate::pipboy::frame::FONTS[n - 1];
            loaded.push((n, crate::interface::face(path, crate::interface::plain_font(32.0, 15.0), data.as_deref(), &mut images, bc)));
        }
        commands.insert_resource(WaitFaces(loaded));
        return;
    };
    for (e, mut g, node) in &mut texts {
        let now = (g.text.clone(), g.color);
        if g.drawn.as_ref() == Some(&now) {
            continue;
        }
        let Some(face) = faces.get(g.font) else { continue };
        // The node's size in interface units (960 to the view's height), at the original's
        // size: the text's own lengths take the UI scale again.
        let size = |v: Val| match v {
            Val::Vh(x) => x * 9.6 / crate::interface::unit_scale(),
            _ => 0.0,
        };
        let (w, h) = (size(node.width), size(node.height));
        let mut entity = commands.entity(e);
        entity.despawn_related::<Children>();
        let width = crate::pad::hint_width(face, &g.text, &glyphs);
        let x = if g.right { w - width } else { (w - width) / 2.0 };
        // The glyphs stand on a line a little below the middle [design].
        let base = h / 2.0 + face.font.size * 0.3;
        entity.with_children(|p| { crate::pad::set_hint(p, face, &g.text, x, base, now.1, &glyphs); });
        g.drawn = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aid_parses() {
        assert_eq!(
            parse_aid("16:open, 17:hours=22,19.5:accept,30:cancel").unwrap(),
            vec![
                (16.0, AidStep::Open),
                (17.0, AidStep::Input(Command::Hours(22))),
                (19.5, AidStep::Input(Command::Accept)),
                (30.0, AidStep::Input(Command::Cancel)),
            ]
        );
        assert_eq!(
            parse_aid("17:point=0.45:0.47,17.5:press,18:release").unwrap(),
            vec![(17.0, AidStep::Point(0.45, 0.47)), (17.5, AidStep::Press), (18.0, AidStep::Release)]
        );
        // The pointer's steps and the others are taken apart, each in time order.
        let mut aid = WaitAid(parse_aid("3:accept,1:press,2:open,0.5:point=0:0,9:release").unwrap());
        assert_eq!(due_steps(&mut aid, 5.0, true), vec![AidStep::Point(0.0, 0.0), AidStep::Press]);
        assert_eq!(due_steps(&mut aid, 5.0, false), vec![AidStep::Open, AidStep::Input(Command::Accept)]);
        assert_eq!(aid.0, vec![(9.0, AidStep::Release)]);
        assert!(parse_aid("16:point=1").is_err());
        assert!(parse_aid("16").is_err());
        assert!(parse_aid("16:sleep").is_err());
        assert!(parse_aid("16:hours=x").is_err());
        assert!(parse_aid("").unwrap().is_empty());
    }

    #[test]
    fn a_held_open_takes_the_later_steps_with_it() {
        let mut aid = WaitAid(parse_aid("1:press,13:cancel,14:open,14:hours=5,16:accept").unwrap());
        // Nothing to hold before the open is due.
        assert_eq!(hold_open(&mut aid, 13.5), None);
        // The steps due before the open go on; those at its time wait with it.
        assert_eq!(hold_open(&mut aid, 14.5), Some((vec![AidStep::Input(Command::Cancel)], 0.5)));
        assert_eq!(hold_open(&mut aid, 15.0), Some((vec![], 0.5)));
        assert_eq!(aid.0, vec![(1.0, AidStep::Press), (15.0, AidStep::Open), (15.0, AidStep::Input(Command::Hours(5))), (17.0, AidStep::Input(Command::Accept))]);
        // In the world again: the open and the step asked at its time are due.
        assert_eq!(due_steps(&mut aid, 15.0, false), vec![AidStep::Open, AidStep::Input(Command::Hours(5))]);
    }

    #[test]
    fn a_sleep_fades_the_screen_out_and_back() {
        let mut f = 0.0;
        // A wait, or a sleep not yet accepted: nothing.
        f = fade_step(f, false, 0.5);
        assert_eq!(f, 0.0);
        f = fade_step(f, true, FADE_SECONDS / 2.0);
        assert!((f - 0.5).abs() < 1e-6);
        f = fade_step(f, true, 10.0);
        assert_eq!(f, 1.0);
        f = fade_step(f, false, FADE_SECONDS / 4.0);
        assert!((f - 0.75).abs() < 1e-6);
        assert_eq!(fade_step(f, false, 10.0), 0.0);
    }

    #[test]
    fn the_marks_fit_the_line() {
        // The end marks stand inside the line's ends.
        let last = MARK_X0 + MARK_STEP * (MAX_HOURS + 1) as f32;
        assert!(MARK_X0 > 65.0 && last < 65.0 + 516.0);
    }

    #[test]
    fn wasd_as_arrow_keys_allows_space_to_accept_sleep_wait_only_when_enabled() {
        let run_space = |wasd: bool| -> Vec<Command> {
            let mut app = App::new();
            let mut tweaks = crate::tweaks::TweakSettings::default();
            tweaks.game.wasd_as_arrow_keys = wasd;
            let mut kb = ButtonInput::<KeyCode>::default();
            kb.press(KeyCode::Space);
            app.insert_resource(kb)
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<crate::bindings::Bindings>()
                .insert_resource(WaitMenu(Some(Model::open(false))))
                .insert_resource(tweaks)
                .add_message::<WaitInput>()
                .add_systems(Update, keys);
            app.update();
            app.world_mut()
                .resource_mut::<bevy::ecs::message::Messages<WaitInput>>()
                .drain()
                .map(|e| e.0)
                .collect()
        };

        assert!(run_space(false).is_empty(), "vanilla Sleep/Wait ignores Space");
        assert_eq!(run_space(true), vec![Command::Accept], "bUseWASDAsArrowKeys accepts with Space");
    }

    #[test]
    fn faster_sleep_wait_shortens_hour_countdown_only_when_enabled() {
        assert_eq!(hour_seconds(false), HOUR_SECONDS);
        assert_eq!(hour_seconds(true), FASTER_HOUR_SECONDS);
        const { assert!(FASTER_HOUR_SECONDS > 0.0 && FASTER_HOUR_SECONDS < HOUR_SECONDS); }

        let mut vanilla = Model::open(false);
        vanilla.apply(Command::Hours(3));
        vanilla.apply(Command::Accept);
        assert!(!vanilla.tick(FASTER_HOUR_SECONDS, hour_seconds(false)), "vanilla pace has not reached 0.5 s yet");
        assert!(vanilla.tick(HOUR_SECONDS - FASTER_HOUR_SECONDS + 0.01, hour_seconds(false)));
        assert_eq!(vanilla.marker(), 2);

        let mut faster = Model::open(true);
        faster.apply(Command::Hours(3));
        faster.apply(Command::Accept);
        assert!(faster.tick(FASTER_HOUR_SECONDS, hour_seconds(true)), "faster pace passes an hour at FASTER_HOUR_SECONDS");
        assert_eq!(faster.marker(), 2);
    }
}
