//! The companion wheel (docs/spec-notes/companion-wheel.md): the menu that opens when the
//! player activates a teammate (`Runtime::talk_after`, `fnv_game::companion_wheel`), laid
//! out by the game's `menus\companion_wheel_menu.xml` and its two prefabs [documented,
//! §2] and drawn with the game's pictures (`textures\interface\companion_wheel\…`).
//!
//! - **What it shows**: the matte and the pie, 760 × 760, centred across and 55 above the
//!   middle of the view; the eight buttons' icons at the file's places (180 × 180), the
//!   picked one's slice lit and its "on" icon, the others' "off" icons; a button's
//!   alternative icons while the companion is in its other state ([`fnv_game::companion_wheel::State::alt`]);
//!   "Companion Commands" at the top, the picked button's words under it in a filled
//!   bar ("Exit" there over the hub), its context lines at the bottom (Heal: the
//!   companion's HP and the stimpaks carried; Open Inventory: its carry weight), Exit in the hub [observed: the owner's
//!   screenshots of 2026-10-19 for the bar and the lines], and the companion's reply under the wheel
//!   (`crate::script_hud`'s subtitle of the companion's line, `ScriptHud::wheel_line`).
//!   A button that cannot be used is drawn at half strength [design].
//! - **Keys**: the pointer picks the slice it is in, 76 to 332 units from the wheel's
//!   centre, by its angle clockwise from straight up [documented: the radials' `user2`
//!   … `user5`; design: measured from the drawn centre, the file's `user0`/`user1` are the
//!   screen's]; a click, Enter or Space uses it; Left / Right (A / D) step round the
//!   buttons; E, Esc or a click on Exit closes it [documented: `_PCButton_E` is the
//!   cancel button; design: the others].
//! - **What a command does**: `fnv_game::game::Game::wheel_command`. The wheel stays open
//!   after a command (the companion's reply is heard and read in it), except Talk (the
//!   conversation opens) and Open Inventory (the container menu opens once it is closed)
//!   [hypothesis].
//! - It is a `PlayState::Menu`: the world and the game clock stand still; MenuMode 1075
//!   runs (`fnv_game::companion_wheel::MENU_CODE`).
//!
//! Test aid `NVF_PLAY_TEST_WHEEL=<seconds>:<step>[,…]`: `open:<ref>` (the player activates
//! the reference, as with E), `open` (again on the companion last opened on), a command by name (`stay_follow`, `aggressive_passive`,
//! `near_far`, `ranged_melee`, `stimpak`, `inventory`, `back_up`, `talk`), `close`. Log
//! lines: `companion wheel: open on <companion> (<buttons>)`, `companion wheel: <command>
//! (<label>): <outcome>`, `companion wheel: closed (<how>)`; at exit, when it was used,
//! `companion wheel summary: …`.

pub mod touch;

use crate::interface::{amber, at, face, plain_font, set_line, Face, Pictures};
use crate::pause::AidTime;
use crate::script::{with_world, ScriptState};
use crate::script_hud::ScriptHud;
use crate::sim::SimState;
use crate::PlayState;
use crate::PlaySet;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use bevy::window::PrimaryWindow;
use fnv_game::companion_wheel::{Command, Outcome, State as WheelButton};
use fnv_sim::FormId;
use nvf_assets::GameData;

// ---- the menu file's numbers [documented: menus\companion_wheel_menu.xml, the prefabs
// button_companion_wheel.xml and icon_companion_wheel.xml, menus\globals.xml] ------------

/// The main rectangle's size, and how far above the middle of the view it stands.
pub const SIZE: f32 = 760.0;
pub const RAISE: f32 = 55.0;
/// The matte reaches 15 beyond the rectangle on each side.
const MATTE: f32 = 15.0;
/// The file's matte: brightness 128 at `_background_fill_alpha` (32 of 255); a lit slice's
/// alpha is the same fill alpha times its `user10` (1 when picked). NVForever draws both
/// stronger, as the screenshots show them ([`MATTE_SHADE`], [`LIT`]).
#[allow(dead_code)]
const FILL_ALPHA: f32 = 32.0 / 255.0;
/// A slice is picked between these distances from the centre.
pub const INNER: f32 = 76.0;
pub const OUTER: f32 = 332.0;
/// The icons' size.
const ICON: f32 = 180.0;
/// Each button: its slice's angles (radians clockwise from up, `user2`, `user3`), its
/// icon's place (`user6`, `user7`: x in the rectangle, y down the 960-high view), the off /
/// on icons (`user8`, `user9`) and the alternative off / on icons (`user12`, `user13`).
pub struct Slice {
    pub from: f32,
    pub to: f32,
    pub icon: [f32; 2],
    pub off: &'static str,
    pub on: &'static str,
    pub alt: Option<(&'static str, &'static str)>,
    pub picture: &'static str,
}

const P: &str = "Interface\\companion_wheel\\";
/// A twelfth of a turn: the file's angles are its multiples, written to six places
/// (0.523598, 1.047196 … 5.759578).
const S: f32 = std::f32::consts::FRAC_PI_6;

pub const SLICES: [Slice; 8] = [
    Slice { from: S, to: 2.0 * S, icon: [467.0, 160.0], off: "Comp_peaceful_off.dds", on: "Comp_peaceful_on.dds", alt: Some(("Comp_aggressive_off.dds", "Comp_aggressive_on.dds")), picture: "hud_comp_slice1.dds" },
    Slice { from: 2.0 * S, to: 3.0 * S, icon: [532.0, 275.0], off: "Comp_stimpack_off.dds", on: "Comp_stimpack_on.dds", alt: None, picture: "hud_comp_slice2.dds" },
    Slice { from: 3.0 * S, to: 4.0 * S, icon: [532.0, 404.0], off: "Comp_follow_off.dds", on: "Comp_follow_on.dds", alt: Some(("Comp_stay_off.dds", "Comp_stay_on.dds")), picture: "hud_comp_slice3.dds" },
    Slice { from: 4.0 * S, to: 5.0 * S, icon: [467.0, 510.0], off: "Comp_talk_off.dds", on: "Comp_talk_on.dds", alt: None, picture: "hud_comp_slice4.dds" },
    Slice { from: 7.0 * S, to: 8.0 * S, icon: [112.0, 500.0], off: "Comp_stepback_off.dds", on: "Comp_stepback_on.dds", alt: None, picture: "hud_comp_slice5.dds" },
    Slice { from: 8.0 * S, to: 9.0 * S, icon: [52.0, 404.0], off: "Comp_stayclose_off.dds", on: "Comp_stayclose_on.dds", alt: Some(("Comp_stayfar_off.dds", "Comp_stayfar_on.dds")), picture: "hud_comp_slice6.dds" },
    Slice { from: 9.0 * S, to: 10.0 * S, icon: [52.0, 275.0], off: "Comp_inven_off.dds", on: "Comp_inven_on.dds", alt: None, picture: "hud_comp_slice7.dds" },
    Slice { from: 10.0 * S, to: 11.0 * S, icon: [122.0, 160.0], off: "Comp_melee_off.dds", on: "Comp_melee_on.dds", alt: Some(("Comp_ranged_off.dds", "Comp_ranged_on.dds")), picture: "hud_comp_slice8.dds" },
];

/// The texts' places, y down the 960-high view: the title (`CWM_RadialLabel`, font 1,
/// wrapped at 230), the picked button's words (`CWM_ButtonPreviewText`, font 3, 190), its
/// context line (`CWM_ButtonContextText`, font 3, 190); all centred across the wheel.
const TITLE_Y: f32 = 110.0;
const TITLE_WRAP: f32 = 230.0;
const PREVIEW_Y: f32 = 205.0;
const CONTEXT_Y: f32 = 570.0;
const LABEL_WRAP: f32 = 190.0;
/// Exit is centred on (770 / 2, 850 / 2) of the view's frame (the hub); the subtitle stands
/// 740 down the rectangle, centred in 800.
const EXIT_AT: [f32; 2] = [385.0, 425.0];
const SUBTITLE_Y: f32 = 740.0;
const SUBTITLE_W: f32 = 800.0;
/// text_box.xml's room round a button's words (as the tutorial menu's).
const HOR_BUF: f32 = 20.0;
const VER_BUF: f32 = 20.0;

/// Font 1 and font 3 [documented: Fallout_default.ini sFontFile_1, sFontFile_3].
const FONT_1: &str = "textures\\fonts\\glow_monofonto_large.fnt";
const FONT_3: &str = "textures\\fonts\\glow_monofonto_medium.fnt";

/// The title's words [documented: the file's comment beside `&-sCWheelTitle;`].
pub const TITLE: &str = "Companion Commands";
/// Exit's words (`&-sExit;`) [hypothesis: not in the masters].
pub const EXIT: &str = "Exit";

// ---- NVForever's own -----------------------------------------------------------------------

/// The bar the picked button's words stand in, under the title: filled amber, the words
/// dark; "Exit" there while the pointer is over the hub [observed: the owner's screenshots
/// of 2026-10-19; the size measured on them, about 250 × 34 units].
const BAR: [f32; 2] = [250.0, 34.0];
/// How far the bar's lower corners keep from the top slice's edges.
const BAR_EDGE_GAP: f32 = 4.0;
/// A picked slice is lit at this strength [observed: fitted by eye to the same screenshot;
/// the file's fill alpha of 32 is fainter than it shows].
const LIT: f32 = 0.3;
/// How dark the matte is drawn [observed: fitted by eye to the owner's screenshots].
const MATTE_SHADE: f32 = 0.8;
/// A button that cannot be used is drawn at this strength [design].
const DISABLED: f32 = 0.5;
/// The buttons are read again this often while it is open, aid seconds (a line's end
/// script changes the companion's variables) [engineering choice].
const REFRESH: f32 = 0.25;

/// The button under a point of the wheel, `p` from the wheel's centre in interface units
/// (y down): by the slices' angles between [`INNER`] and [`OUTER`]; `Err(())` for the hub
/// (Exit), `Ok(None)` for nothing.
#[allow(clippy::result_unit_err)]
pub fn hit(p: Vec2) -> Result<Option<usize>, ()> {
    let r = p.length();
    if r < INNER {
        return Err(());
    }
    if r > OUTER {
        return Ok(None);
    }
    let mut a = p.x.atan2(-p.y);
    if a < 0.0 {
        a += std::f32::consts::TAU;
    }
    Ok(SLICES.iter().position(|s| (s.from..s.to).contains(&a)))
}

/// The next usable button from `from`, stepping `by` (±1) round the wheel.
pub fn step(buttons: &[WheelButton], from: Option<usize>, by: i32) -> Option<usize> {
    let n = buttons.len() as i32;
    if n == 0 {
        return None;
    }
    let mut i = from.map_or(if by > 0 { -1 } else { n }, |f| f as i32);
    for _ in 0..n {
        i = (i + by).rem_euclid(n);
        if buttons[i as usize].enabled {
            return Some(i as usize);
        }
    }
    from
}

/// What the player did on the wheel.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelInput {
    Use(Command),
    Close,
}

/// The open wheel.
pub struct Session {
    pub companion: FormId,
    pub name: String,
    pub buttons: Vec<WheelButton>,
    /// Reasons read from the same rules as button eligibility.
    pub unavailable: Vec<Option<String>>,
    /// The button picked (under the pointer, or stepped to).
    pub selected: Option<usize>,
    /// The pointer is over Exit.
    pub exit_hover: bool,
    /// Opened this frame: its keys are the activation's.
    fresh: bool,
    refreshed: f32,
}

#[derive(Resource, Default)]
pub struct WheelScreen(pub Option<Session>);

#[derive(Resource, Default)]
struct WheelStats {
    opened: usize,
    commands: Vec<String>,
}

#[derive(Resource, Default)]
struct WheelAid {
    steps: Vec<(f32, AidStep)>,
    /// The companion the aid last opened the wheel on (a bare `open` opens it again).
    last: Option<FormId>,
}

#[derive(Clone, Debug, PartialEq)]
enum AidStep {
    Open(String),
    Input(WheelInput),
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let step = match what.split_once(':') {
                Some(("open", r)) if !r.is_empty() => AidStep::Open(r.to_string()),
                // Again on the companion it was last opened on.
                None if what == "open" => AidStep::Open(String::new()),
                None if what == "close" => AidStep::Input(WheelInput::Close),
                None => AidStep::Input(WheelInput::Use(Command::parse(what).ok_or_else(|| format!("{what:?}: expected open[:<ref>], close or a command"))?)),
                _ => return Err(format!("{what:?}: expected open[:<ref>], close or a command")),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct CompanionWheelPlugin;

impl Plugin for CompanionWheelPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_WHEEL").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_WHEEL: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<WheelScreen>()
            .init_resource::<WheelStats>()
            .init_resource::<WheelView>()
            .init_resource::<touch::Settings>()
            .init_resource::<touch::TouchState>()
            .add_systems(Startup, touch::load)
            .insert_resource(WheelAid { steps, last: None })
            .add_message::<WheelInput>()
            .add_systems(Update, (run_aid, open, follow_game, touch::input, (keys, apply).chain().run_if(in_state(PlayState::Menu)), draw).chain().in_set(PlaySet::Ui))
            .add_systems(Update, touch::draw.after(draw).in_set(PlaySet::Ui))
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
        #[cfg(any(target_os = "android", test))]
        touch::register(app);
    }
}

fn run_aid(time: AidTime, mut aid: ResMut<WheelAid>, mut input: MessageWriter<WheelInput>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).cloned().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_WHEEL: {step:?}");
        match step {
            AidStep::Open(who) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                let last = aid.last;
                let r = with_world(rt, &mut sim, |rt| {
                    let r = if who.is_empty() { last.ok_or_else(|| "a bare open needs an earlier open:<ref>".to_string())? } else { fnv_game::runner::resolve(&rt.game, &who)? };
                    rt.activate_as_player(r).map(|done| (r, matches!(done, crate::script::ByHand::Wheel)))
                });
                match r {
                    Ok((r, true)) => aid.last = Some(r),
                    Ok((r, false)) => warn!("test aid NVF_PLAY_TEST_WHEEL: activating {who} ({r}) opened no companion wheel"),
                    Err(e) => warn!("test aid NVF_PLAY_TEST_WHEEL: {who}: {e}"),
                }
            }
            AidStep::Input(i) => {
                input.write(i);
            }
        }
    }
}

fn describe(buttons: &[WheelButton]) -> String {
    buttons
        .iter()
        .map(|b| format!("{}{}", b.label, if b.enabled { "" } else { " (off)" }))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The runtime opened a wheel (the player's activation): the menu takes the screen.
fn open(
    mut screen: ResMut<WheelScreen>,
    mut script: Option<ResMut<ScriptState>>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    time: AidTime,
    mut stats: ResMut<WheelStats>,
    mut sim: ResMut<SimState>,
) {
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    let Some(c) = rt.game.wheel_companion() else { return };
    if screen.0.as_ref().is_some_and(|s| s.companion == c) {
        return;
    }
    let (buttons, unavailable) = with_world(rt, &mut sim, |rt| {
        let buttons = rt.game.wheel_buttons(c);
        let unavailable = buttons.iter().map(|b| rt.game.wheel_unavailable_reason_for(c, b)).collect();
        (buttons, unavailable)
    });
    let base = sim.base_of(c.0).unwrap_or(c);
    let name = sim.name(base).map(str::to_string).unwrap_or_else(|| rt.game.data.name(c));
    info!("companion wheel: open on {} ({}) (MenuMode {}): {}", rt.game.data.name(c), name, fnv_game::companion_wheel::MENU_CODE, describe(&buttons));
    stats.opened += 1;
    screen.0 = Some(Session { companion: c, name, buttons, unavailable, selected: None, exit_hover: false, fresh: true, refreshed: time.elapsed_secs() });
    if *state.get() != PlayState::Menu {
        next.set(PlayState::Menu);
    }
}

/// The pointer, the keys and clicks.
fn keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    mouse: crate::pad::ActionButtons<MouseButton>,
    windows: Query<&Window, With<PrimaryWindow>>,
    exit: Query<&PointerState, (Changed<PointerState>, With<ExitButton>)>,
    (mut screen, touch): (ResMut<WheelScreen>, Option<Res<touch::Settings>>),
    mut out: MessageWriter<WheelInput>,
    mut last_pointer: Local<Option<Vec2>>,
) {
    let keys = keys.snapshot();
    let mouse = mouse.snapshot();
    let pointer_enabled = touch.is_none_or(|settings| !settings.enabled);
    use KeyCode::*;
    let Some(s) = screen.0.as_mut() else { return };
    if s.fresh {
        s.fresh = false;
        return;
    }
    // The pointer in interface units from the wheel's centre.
    if let Some((p, size)) = windows.single().ok().filter(|_| pointer_enabled).and_then(|w| Some((w.cursor_position()?, Vec2::new(w.width(), w.height())))) {
        // Where the pointer is on the wheel as drawn at the UI scale (#1631).
        let p = crate::interface::from_menu_frame(p, size, crate::interface::MIDDLE);
        let k = 960.0 / size.y.max(1.0);
        let centre = Vec2::new(size.x * k / 2.0, 480.0 - RAISE);
        let moved = last_pointer.is_none_or(|before| before != p);
        *last_pointer = Some(p);
        if moved { match hit(p * k - centre) {
            Ok(Some(i)) => {
                if s.selected != Some(i) {
                    s.selected = Some(i);
                }
                s.exit_hover = false;
            }
            Ok(None) => s.exit_hover = false,
            Err(()) => s.exit_hover = true,
        } }
    }
    if (pointer_enabled && exit.iter().any(|i| *i == PointerState::Pressed)) || keys.any_just_pressed([KeyE, Escape]) {
        out.write(WheelInput::Close);
        return;
    }
    if keys.any_just_pressed([ArrowRight, KeyD]) {
        s.selected = step(&s.buttons, s.selected, 1);
    } else if keys.any_just_pressed([ArrowLeft, KeyA]) {
        s.selected = step(&s.buttons, s.selected, -1);
    }
    let clicked = pointer_enabled && mouse.just_pressed(MouseButton::Left) && !s.exit_hover;
    if clicked || keys.any_just_pressed([Enter, NumpadEnter, Space]) {
        if let Some(b) = s.selected.and_then(|i| s.buttons.get(i)) {
            out.write(WheelInput::Use(b.command));
        }
    }
}

/// Carry out what the player did.
#[allow(clippy::too_many_arguments)]
fn apply(
    mut input: MessageReader<WheelInput>,
    mut screen: ResMut<WheelScreen>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    mut next: ResMut<NextState<PlayState>>,
    time: AidTime,
    mut stats: ResMut<WheelStats>,
) {
    let inputs: Vec<WheelInput> = input.read().copied().collect();
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    if let Some(i) = inputs.into_iter().next() {
        let Some(s) = screen.0.as_mut() else { return };
        match i {
            WheelInput::Close => {
                rt.game.close_companion_wheel();
                info!("companion wheel: closed (the player)");
                screen.0 = None;
                next.set(PlayState::Playing);
            }
            WheelInput::Use(command) => {
                let label = s.buttons.iter().find(|b| b.command == command).map_or("?", |b| b.label);
                let c = s.companion;
                let r = with_world(rt, &mut sim, |rt| rt.game.wheel_command(command));
                let g = &rt.game;
                let said = |info: FormId| format!("{} says {}", g.data.name(c), g.data.name(info));
                let line = match &r {
                    Ok(Outcome::Said { topic, info }) => format!("{} ({})", said(*info), g.data.name(*topic)),
                    Ok(Outcome::NoLine { topic }) => format!("{} has no line of {topic} to say: nothing happens", g.data.name(c)),
                    Ok(Outcome::Talk { info }) => format!("the conversation starts with {}", g.data.name(*info)),
                    Ok(Outcome::Stimpak { item, line }) => format!("{} used on {}{}", g.data.name(*item), g.data.name(c), line.map_or_else(String::new, |l| format!(": {}", said(l)))),
                    Ok(Outcome::BackUp) => format!("{} is told to back up", g.data.name(c)),
                    Ok(Outcome::Refused) => "the button cannot be used now".to_string(),
                    Err(e) => format!("failed: {e}"),
                };
                info!("companion wheel: {} ({label}): {line}", command.name());
                stats.commands.push(command.name().to_string());
                // Talk: the conversation has the screen now (`crate::dialogue`).
                if matches!(r, Ok(Outcome::Talk { .. })) {
                    screen.0 = None;
                    return;
                }
                // Open Inventory: the line asked for the container (OpenTeammateContainer),
                // which opens once the wheel is closed (`crate::menu`).
                if command == Command::OpenInventory && matches!(r, Ok(Outcome::Said { .. })) {
                    rt.game.close_companion_wheel();
                    info!("companion wheel: closed (the companion's inventory opens)");
                    screen.0 = None;
                    next.set(PlayState::Playing);
                    return;
                }
                let (buttons, unavailable) = with_world(rt, &mut sim, |rt| {
                    let buttons = rt.game.wheel_buttons(c);
                    let unavailable = buttons.iter().map(|b| rt.game.wheel_unavailable_reason_for(c, b)).collect();
                    (buttons, unavailable)
                });
                s.buttons = buttons;
                s.unavailable = unavailable;
                s.refreshed = time.elapsed_secs();
                // The command took effect as the reply started (`Game::wheel_command`).
                if let Some(b) = s.buttons.iter().find(|b| b.command == command) {
                    info!("companion wheel: {} now shows {}", command.name(), b.label);
                }
                // All producers share one actual command per frame; the full input
                // batch was already drained, so duplicate taps cannot run next frame.
            }
        }
    }
}

/// The runtime closed the wheel (a conversation, CloseAllMenus, the companion gone) or its
/// companion's state changed: the screen follows.
fn follow_game(mut screen: ResMut<WheelScreen>, mut script: Option<ResMut<ScriptState>>, mut sim: ResMut<SimState>, state: Res<State<PlayState>>, mut next: ResMut<NextState<PlayState>>, time: AidTime) {
    let Some(s) = screen.0.as_mut() else { return };
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    let open = rt.game.wheel_companion() == Some(s.companion);
    if !open || !rt.game.is_teammate(s.companion) {
        rt.game.close_companion_wheel();
        info!("companion wheel: closed (by the game)");
        screen.0 = None;
        if *state.get() == PlayState::Menu && rt.game.dialogue.is_none() {
            next.set(PlayState::Playing);
        }
        return;
    }
    let now = time.elapsed_secs();
    if now - s.refreshed >= REFRESH {
        s.refreshed = now;
        let c = s.companion;
        let (b, unavailable) = with_world(rt, &mut sim, |rt| {
            let b = rt.game.wheel_buttons(c);
            let u = b.iter().map(|btn| rt.game.wheel_unavailable_reason_for(c, btn)).collect();
            (b, u)
        });
        if b != s.buttons {
            s.buttons = b;
        }
        s.unavailable = unavailable;
    }
}

fn close(mut screen: ResMut<WheelScreen>, script: Option<ResMut<ScriptState>>) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    if screen.0.take().is_some() && rt.game.close_companion_wheel() {
        info!("companion wheel: closed (another state)");
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<WheelStats>) {
    if exit.read().next().is_none() || stats.opened == 0 {
        return;
    }
    info!("companion wheel summary: opened {} times; commands: {}", stats.opened, if stats.commands.is_empty() { "none".to_string() } else { stats.commands.join(", ") });
}

// ---- drawing -------------------------------------------------------------------------------

#[derive(Component)]
struct WheelRoot;

/// The hub's Exit.
#[derive(Component)]
struct ExitButton;

/// The fonts and pictures, read when first drawn.
struct Kit {
    font1: Face,
    font3: Face,
    pictures: Pictures,
}

#[derive(Resource, Default)]
struct WheelView {
    kit: Option<Kit>,
}

/// Text centred on `cx`, its first line's top at `top`, wrapped at `wrap`.
#[allow(clippy::too_many_arguments)]
fn centred(p: &mut ChildSpawnerCommands, face: &Face, s: &str, cx: f32, top: f32, wrap: Option<f32>, color: Color) -> f32 {
    let base = crate::interface::line_base(face);
    let lines = face.font.lines(s.trim(), wrap);
    let mut widest: f32 = 0.0;
    for (k, line) in lines.iter().enumerate() {
        widest = widest.max(line.width);
        set_line(p, face, line, cx - line.width / 2.0, top + base + face.font.size * k as f32, color);
    }
    widest
}

#[allow(clippy::too_many_arguments)]
fn picture(p: &mut ChildSpawnerCommands, kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, name: &str, node: Node, color: Color) {
    if let Some((image, rect)) = kit.pictures.picture(&format!("{P}{name}"), data, images, bc) {
        p.spawn((node, ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }));
    }
}

/// What was drawn last.
type Shown = (Vec<WheelButton>, Vec<Option<String>>, Option<usize>, bool, Option<String>, i32);

#[derive(Default, PartialEq)]
struct Drawn(Option<Shown>);

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    screen: Res<WheelScreen>,
    hud: Res<ScriptHud>,
    roots: Query<Entity, With<WheelRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut view: ResMut<WheelView>,
    mut shown: Local<Drawn>,
) {
    let width = windows.single().map_or(1280.0, |w| 960.0 * w.width() / w.height().max(1.0));
    let now = Drawn(screen.0.as_ref().map(|s| (s.buttons.clone(), s.unavailable.clone(), s.selected, s.exit_hover, hud.wheel_line.clone(), width.round() as i32)));
    if now == *shown {
        return;
    }
    *shown = now;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(s) = screen.0.as_ref() else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let data = data.as_deref();
    let kit = view.kit.get_or_insert_with(|| Kit {
        font1: face(FONT_1, plain_font(31.0, 12.0), data, &mut images, bc),
        font3: face(FONT_3, plain_font(25.0, 10.0), data, &mut images, bc),
        pictures: Pictures::load(data),
    });
    let left = (width - SIZE) / 2.0;
    let top = (960.0 - SIZE) / 2.0 - RAISE;
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let bright = amber(255.0);
    // At the UI scale the wheel keeps to the view's middle (#1631).
    commands.spawn((WheelRoot, crate::interface::framed_root(full, crate::interface::MIDDLE), GlobalZIndex(106))).with_children(|r| {
        r.spawn(at(left, top, SIZE, SIZE)).with_children(|m| {
            // The matte darkens the world inside the wheel [observed: dark behind the pie on
            // the owner's screenshots of 2026-10-19; the file's brightness 128 at fill
            // alpha 32 would hardly show].
            let matte = Color::BLACK.with_alpha(MATTE_SHADE);
            picture(m, kit, data, &mut images, bc, "hud_comp_bkg.dds", at(-MATTE, -MATTE, SIZE + 2.0 * MATTE, SIZE + 2.0 * MATTE), matte);
            picture(m, kit, data, &mut images, bc, "hud_comp_pie.dds", at(0.0, 0.0, SIZE, SIZE), bright);
            for (k, (slice, b)) in SLICES.iter().zip(&s.buttons).enumerate() {
                let picked = s.selected == Some(k);
                let strength = if b.enabled { 1.0 } else { DISABLED };
                if picked {
                    picture(m, kit, data, &mut images, bc, slice.picture, at(0.0, 0.0, SIZE, SIZE), bright.with_alpha(LIT));
                }
                let (off, on) = match slice.alt {
                    Some(alt) if b.alt => alt,
                    _ => (slice.off, slice.on),
                };
                let icon = if picked { on } else { off };
                picture(m, kit, data, &mut images, bc, icon, at(slice.icon[0], slice.icon[1] - top, ICON, ICON), bright.with_alpha(strength));
            }
            let cx = SIZE / 2.0;
            centred(m, &kit.font1, TITLE, cx, TITLE_Y - top, Some(TITLE_WRAP), bright);
            let picked = s.selected.and_then(|i| s.buttons.get(i));
            let words = if s.exit_hover { Some((EXIT, true)) } else { picked.map(|b| (b.label, b.enabled)) };
            if let Some((words, enabled)) = words {
                let bar_top = PREVIEW_Y - top - (BAR[1] - kit.font3.font.size) / 2.0;
                // Narrowed to stay inside the empty top slice (±30° from up): at 250 its lower
                // corners crossed the slices' edges [observed: owner's screenshot 2026-10-27].
                let room = 2.0 * ((cx - (bar_top + BAR[1])) * S.tan() - BAR_EDGE_GAP);
                let bar_w = BAR[0].min(room);
                m.spawn((at(cx - bar_w / 2.0, bar_top, bar_w, BAR[1]), BackgroundColor(bright.with_alpha(if enabled { 1.0 } else { DISABLED }))));
                centred(m, &kit.font3, words, cx, PREVIEW_Y - top, Some(LABEL_WRAP), Color::BLACK);
            }
            if let Some(b) = picked.filter(|_| !s.exit_hover) {
                if let Some(ctx) = &b.context {
                    let color = bright.with_alpha(if b.enabled { 1.0 } else { DISABLED });
                    for (k, l) in ctx.lines().enumerate() {
                        centred(m, &kit.font3, l, cx, CONTEXT_Y - top + kit.font3.font.size * k as f32, Some(LABEL_WRAP), color);
                    }
                }
            }
            if let Some(reason) = s.selected.and_then(|i| s.unavailable.get(i)).and_then(Option::as_deref) {
                centred(m, &kit.font3, reason, cx, CONTEXT_Y - top + 72.0, Some(LABEL_WRAP), bright);
            }
            // Exit in the hub (text_box.xml).
            let w = kit.font1.font.lines(EXIT, None).first().map_or(0.0, |l| l.width) + HOR_BUF;
            let h = kit.font1.font.size + VER_BUF;
            let (x, y) = (EXIT_AT[0] - w / 2.0, EXIT_AT[1] - h / 2.0 - top);
            centred(m, &kit.font1, EXIT, x + w / 2.0, y + VER_BUF / 2.0, None, bright);
            m.spawn((Button, ExitButton, at(x, y, w, h)));
            // The companion's reply.
            if let Some(line) = &hud.wheel_line {
                centred(m, &kit.font1, line, SUBTITLE_W / 2.0, SUBTITLE_Y, Some(SUBTITLE_W), bright);
            }
        });
    });
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> WheelScreen {
    WheelScreen(Some(Session {
        companion: FormId(42),
        name: "Synthetic companion".into(),
        buttons: vec![WheelButton { command: Command::Talk, alt: false, enabled: true, label: "Synthetic command", context: None }],
        unavailable: vec![None],
        selected: (step >= 1).then_some(0),
        exit_hover: step >= 2,
        fresh: false,
        refreshed: step as f32,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(c: Command, enabled: bool) -> WheelButton {
        WheelButton { command: c, alt: false, enabled, label: c.label(false), context: None }
    }

    #[test]
    fn the_slices_follow_the_file() {
        // Ids in the file's order, as the rules number them.
        for (k, c) in Command::ALL.iter().enumerate() {
            assert_eq!(c.id(), k);
            assert_eq!(Command::from_id(k), Some(*c));
        }
        // Each icon's middle lies in its own slice, measured from the rectangle's middle
        // (the icons' y is down the view, the rectangle 45 down it).
        let top = (960.0 - SIZE) / 2.0 - RAISE;
        assert_eq!(top, 45.0);
        for (k, s) in SLICES.iter().enumerate() {
            let mid = Vec2::new(s.icon[0] + ICON / 2.0 - SIZE / 2.0, s.icon[1] - top + ICON / 2.0 - SIZE / 2.0);
            assert_eq!(hit(mid), Ok(Some(k)), "icon {k} at {mid:?}");
        }
    }

    #[test]
    fn the_pointer_picks_by_angle_and_distance() {
        // Up and down are no button's (the title and the context line are there).
        assert_eq!(hit(Vec2::new(0.0, -200.0)), Ok(None));
        assert_eq!(hit(Vec2::new(0.0, 200.0)), Ok(None));
        // Right of the centre, a little up: Use StimPak (60° to 90°).
        assert_eq!(hit(Vec2::new(200.0, -20.0)), Ok(Some(1)));
        // A little down: Stay / Follow (90° to 120°).
        assert_eq!(hit(Vec2::new(200.0, 20.0)), Ok(Some(2)));
        // Left, a little up: Open Inventory (270° to 300°).
        assert_eq!(hit(Vec2::new(-200.0, -20.0)), Ok(Some(6)));
        // The hub is Exit; beyond the rim nothing.
        assert_eq!(hit(Vec2::new(10.0, 10.0)), Err(()));
        assert_eq!(hit(Vec2::new(400.0, 0.0)), Ok(None));
    }

    #[test]
    fn the_keys_step_over_buttons_that_cannot_be_used() {
        let b: Vec<WheelButton> = Command::ALL.iter().map(|&c| state(c, c != Command::UseStimpak)).collect();
        assert_eq!(step(&b, None, 1), Some(0));
        assert_eq!(step(&b, Some(0), 1), Some(2));
        assert_eq!(step(&b, Some(2), -1), Some(0));
        assert_eq!(step(&b, Some(0), -1), Some(7));
        assert_eq!(step(&b, None, -1), Some(7));
    }

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("10:open:CraigBooneREF,12:stay_follow,13:stimpak,20:close,30:open").unwrap();
        assert_eq!(s[4].1, AidStep::Open(String::new()));
        assert_eq!(s[0], (10.0, AidStep::Open("CraigBooneREF".into())));
        assert_eq!(s[1].1, AidStep::Input(WheelInput::Use(Command::StayFollow)));
        assert_eq!(s[2].1, AidStep::Input(WheelInput::Use(Command::UseStimpak)));
        assert_eq!(s[3].1, AidStep::Input(WheelInput::Close));
        assert!(parse_aid("1:dance").is_err());
    }

    #[test]
    fn the_smoke_scenarios_steps_parse() {
        // Every NVF_PLAY_TEST_WHEEL value in tools/smoke/scenarios.toml (a bare `open` there
        // once made the whole list fail at run time).
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/smoke/scenarios.toml")).unwrap();
        let mut n = 0;
        for part in text.split("NVF_PLAY_TEST_WHEEL = \"").skip(1) {
            let value = part.split('"').next().unwrap();
            parse_aid(value).unwrap_or_else(|e| panic!("{value:?}: {e}"));
            n += 1;
        }
        assert!(n > 0, "the scenario uses the aid");
    }
}
