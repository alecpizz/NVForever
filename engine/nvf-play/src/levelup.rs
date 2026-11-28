//! The level-up menu (1027, `menus\levelup_menu.xml`), drawn as the original
//! (docs/spec-notes/levelup.md): "WELCOME TO LEVEL n" over the skill list, each row with
//! its value and "+" / "−" arrows, the chosen skill's picture and description at the
//! right, "ASSIGN n SKILL POINTS" under them, Reset and Done at the top right; at a perk
//! level Continue turns to a second page of perks. The rules (points, limits, what Done
//! writes) are `fnv_game::levelup`'s; this module draws the menu and turns keys and
//! clicks into changes of its sheets ([`model`]).
//!
//! - The menu opens when the runtime's open menu (`Game::menu`) becomes 1027: from the
//!   console's `advlevel` (`crate::console`), from `fnv_game::levelup::advance` /
//!   `open` (what levels the player by experience calls), or the test aid. It reads the
//!   player as the game holds them then.
//! - Done writes the sheets into the game and answers the menu (`chargen::finish`).
//!   Continue to the perks page writes the skill points already (perk requirements read
//!   the raised skills); Back takes them out again.
//! - Keys [hypothesis: the original's PC keys besides the hints are not documented]:
//!   Up/Down (or W/S, the wheel) choose a row, Page Up/Down a page, Right / Left raise
//!   and lower the chosen skill (held, they repeat [documented: the row's
//!   `repeathorizontal`]), Enter or Space raise it (on the perks page take or give back
//!   the chosen perk), R resets, A is Continue / Done, E is Back [documented: the menu's
//!   `_PCButton_R`, `_PCButton_A`, `_PCButton_E`]. The pointer chooses the row it is on;
//!   a click on "+" or "−" raises or lowers that row, on a perk takes it; Reset,
//!   Continue / Done, Back and the scroll arrows are clickable.
//! - Test aid `NVF_PLAY_TEST_LEVELUP=<seconds>:<op>[,...]` (on the aids' clock):
//!   `advance` (as `advlevel`), `open` (the menu for the level the player has),
//!   `raise:<skill>[:<times>]`, `lower:<skill>`, `pick:<perk>`, `reset`, `continue` /
//!   `done` (A), `back` (E).

pub mod draw;
pub mod model;

use crate::chargen;
use crate::pause::AidTime;
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::PlaySet;
use bevy::image::CompressedImageFormatSupport;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::ui_pointer::PointerState;
use fnv_game::game::menus;
use fnv_game::levelup as rules;
use fnv_game::Game;
use model::{LevelUp, Page, Part, Row};
use nvf_assets::GameData;

/// Whether this module draws the menu with this code.
pub fn draws(code: u16) -> bool {
    code == menus::LEVEL_UP
}

/// Run `f` on the game with the simulation's world in it.
fn with_game<R>(script: &mut ScriptState, sim: &mut SimState, f: impl FnOnce(&mut Game) -> R) -> Option<R> {
    let rt = script.rt.as_mut()?;
    Some(crate::script::with_world(rt, sim, |rt| f(&mut rt.game)))
}

#[derive(Resource, Default)]
pub struct LevelUpMenu {
    open: Option<LevelUp>,
    /// Redraw on the next frame.
    dirty: bool,
    /// Done was pressed; the runtime closes the menu after this frame's scripts.
    answered: bool,
    kit: Option<chargen::draw::Kit>,
}

impl LevelUpMenu {
    /// [design, #2403 / file refs] Read-only page, list position and allocation
    /// progress (`model::LevelUp`); hover and redraw flags do not count.
    pub fn progress(&self) -> Option<impl std::fmt::Debug + '_> {
        self.open.as_ref().map(|m| (m.level, m.page, m.selected, m.scroll, &m.skills.spent, &m.perks, self.answered))
    }
}

/// [design, #2403] Synthetic navigation/allocation states for reports tests only.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> LevelUpMenu {
    let mut m = LevelUp::new(2, rules::SkillSheet::new(vec![32, 33], vec![10, 10], vec![1, 1], 5), true);
    if step >= 1 {
        m.selected = 1;
    }
    if step >= 2 {
        m.skills.spent[1] = 1;
    }
    if step >= 3 {
        m.to_perks(rules::PerkSheet { perks: (1..=20).map(fnv_sim::FormId).collect(), available: vec![true; 20], chosen: Default::default(), count: 1 });
    }
    if step >= 4 {
        m.scroll_by(1);
    }
    if step >= 5 {
        m.perks.as_mut().unwrap().chosen.insert(fnv_sim::FormId(1));
    }
    if step >= 6 {
        m.hover = Some(Part::Continue);
    }
    LevelUpMenu { open: Some(m), dirty: step != 6, ..Default::default() }
}

#[derive(Component)]
struct LevelUpRoot;

/// A change asked for by keys, the pointer or the test aid.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    Step(i32),
    Select(usize),
    Scroll(i32),
    /// Raise or lower a row (the chosen one when `None`).
    Raise(Option<usize>),
    Lower(Option<usize>),
    /// Enter: raise the chosen skill, or take the chosen perk.
    Enter,
    /// A click on a row: on the perks page it takes the perk.
    Click(usize),
    Reset,
    Continue,
    Back,
    /// Test aid: as `advlevel`, or open the menu for the level the player has.
    Advance,
    Open,
    /// Test aid: the row of that name, then an op on it.
    Named(String, Box<Op>),
}

/// `NVF_PLAY_TEST_LEVELUP`: the steps still to take, by seconds.
#[derive(Resource, Default)]
struct LevelUpAid(Vec<(f32, Op)>);

/// Read `NVF_PLAY_TEST_LEVELUP` (`<seconds>:<op>[,...]`).
fn parse_aid(s: &str) -> Result<Vec<(f32, Op)>, String> {
    let mut out = Vec::new();
    for item in s.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (t, rest) = item.split_once(':').ok_or_else(|| format!("{item:?}: expected <seconds>:<op>"))?;
        let t: f32 = t.trim().parse().map_err(|_| format!("{item:?}: bad seconds"))?;
        let mut parts = rest.split(':');
        let head = parts.next().unwrap_or_default();
        let name = parts.next().map(str::to_string);
        let times: usize = match parts.next() {
            Some(n) => n.parse().map_err(|_| format!("{item:?}: bad count"))?,
            None => 1,
        };
        let ops: Vec<Op> = match (head, name) {
            ("advance", None) => vec![Op::Advance],
            ("open", None) => vec![Op::Open],
            ("reset", None) => vec![Op::Reset],
            ("continue" | "done", None) => vec![Op::Continue],
            ("back", None) => vec![Op::Back],
            ("raise", Some(n)) => vec![Op::Named(n, Box::new(Op::Raise(None))); times],
            ("lower", Some(n)) => vec![Op::Named(n, Box::new(Op::Lower(None))); times],
            ("pick", Some(n)) => vec![Op::Named(n, Box::new(Op::Enter))],
            _ => return Err(format!("{item:?}: unknown step")),
        };
        out.extend(ops.into_iter().map(|op| (t, op)));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

pub struct LevelUpPlugin;

impl Plugin for LevelUpPlugin {
    fn build(&self, app: &mut App) {
        let aid = match std::env::var("NVF_PLAY_TEST_LEVELUP") {
            Ok(v) => parse_aid(&v).unwrap_or_else(|e| {
                warn!("NVF_PLAY_TEST_LEVELUP: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        app.init_resource::<LevelUpMenu>().insert_resource(LevelUpAid(aid)).add_systems(Update, (sync, input, draw_menu).chain().in_set(PlaySet::Ui).after(chargen::ChargenSet));
    }
}

/// Open the menu when the game opens it, close it when the game closes it.
fn sync(mut st: ResMut<LevelUpMenu>, script: Option<ResMut<ScriptState>>, sim: Option<ResMut<SimState>>, mut sounds: MessageWriter<UiSound>) {
    let (Some(mut script), Some(mut sim)) = (script, sim) else { return };
    let open = chargen::open_code(&script).is_some_and(draws);
    if st.open.is_some() == open {
        if open && !st.answered {
            script_asks(&mut st, &mut script, &mut sim, &mut sounds);
        }
        return;
    }
    st.dirty = true;
    st.answered = false;
    if !open {
        st.open = None;
        with_game(&mut script, &mut sim, |g| g.showoff.b5.levelup = Default::default());
        info!("levelup: menu closed");
        sounds.write(UiSound { menu: UiMenu::LevelUp, event: UiEvent::Closed });
        return;
    }
    st.open = with_game(&mut script, &mut sim, |g| {
        let level = g.player_level() as i32;
        let mut m = LevelUp::open(g, level);
        // ShowPerkMenu / ShowSkillMenu: the perks page alone, or the skills page as the
        // last, with their counts and title (`fnv_game::nvse_showoff5`).
        if let Some(ask) = g.showoff.b5.levelup.open_as.take() {
            m.title = ask.title;
            if ask.perks_only {
                let mut p = rules::PerkSheet::of_player(g, level);
                p.count = ask.count.unwrap_or(1).max(0) as usize;
                m.perk_due = true;
                m.perks_only = true;
                m.to_perks(p);
                info!("levelup: opened on the perks page alone (ShowPerkMenu), {} to take", ask.count.unwrap_or(1));
            } else {
                m.perk_due = false;
                if let Some(n) = ask.count {
                    m.skills.points = n.max(0);
                }
                info!("levelup: opened on the skills page alone (ShowSkillMenu), {} points", m.skills.points);
            }
        }
        g.showoff.b5.levelup.can_exit_early = false;
        m.preview_perks = g.tweaks.preview_perks && !m.perks_only && !m.perk_due;
        info!("levelup: menu open, level {level}, {} skill points, perk {}", m.skills.points, if m.perk_due { "due" } else { "not due" });
        // The player's tweak: nothing to spend on skills, so straight to the perks [design: docs/design/tweaks.md].
        if g.tweaks.skip_skill_page_without_points && !m.perks_only && m.skills.points == 0 && m.perk_due {
            info!("levelup: no skill points to spend, the perks page first (tweak)");
            m.to_perks(rules::PerkSheet::of_player(g, level));
        }
        m
    });
    sounds.write(UiSound { menu: UiMenu::LevelUp, event: UiEvent::Opened });
}

/// Each frame the menu is open: the page and the points go to the game (ShowOff's
/// GetLevelUpMenuCurrentPage, GetLevelUpMenuPoints), and a page a script asks for
/// (SetLevelUpMenuCurrentPage) is turned to: 1 as Continue, which closes the menu when no
/// perk is due, 0 as Back, 2 as Done, each whatever points are left (`nvse_showoff5`).
fn script_asks(st: &mut LevelUpMenu, script: &mut ScriptState, sim: &mut SimState, sounds: &mut MessageWriter<UiSound>) {
    let Some(m) = st.open.as_ref() else { return };
    let page = match m.page {
        Page::Skills => 0u8,
        Page::Perks => 1,
    };
    let perks_max = m.perks.as_ref().map_or(if m.perk_due { rules::PERKS_PER_LEVEL as i32 } else { 0 }, |p| p.count as i32);
    let points = [m.skills.points, m.skills.points - m.skills.remaining(), perks_max, m.perks.as_ref().map_or(0, |p| p.chosen.len() as i32)];
    let ask = with_game(script, sim, |g| {
        let l = &mut g.showoff.b5.levelup;
        l.page = Some(page);
        l.points = points;
        l.page_request.take()
    })
    .flatten();
    let event = match (ask, page) {
        (Some(1), 0) | (Some(2), _) => Some(press(st, script, sim, "a script (SetLevelUpMenuCurrentPage)", true, ask == Some(2))),
        (Some(0), 1) => apply(st, script, sim, Op::Back, "a script (SetLevelUpMenuCurrentPage)"),
        _ => None,
    };
    if let Some(event) = event {
        st.dirty = true;
        sounds.write(UiSound { menu: UiMenu::LevelUp, event });
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn input(
    mut st: ResMut<LevelUpMenu>,
    mut aid: ResMut<LevelUpAid>,
    time: AidTime,
    keys: crate::pad::ActionButtons<KeyCode>,
    (mut typed, switching): (MessageReader<KeyboardInput>, Option<Res<crate::pad::SwitchingInput>>),
    mut wheel: MessageReader<MouseWheel>,
    rows: Query<(&PointerState, &Row), Changed<PointerState>>,
    parts: Query<(&PointerState, &Part), Changed<PointerState>>,
    console: Res<crate::console::Console>,
    script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<SimState>>,
    mut sounds: MessageWriter<UiSound>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let keys = keys.snapshot();
    use KeyCode::*;
    let typed: Vec<KeyboardInput> = typed.read().filter(|e| switching.as_ref().is_none_or(|s| s.allows(e))).cloned().collect();
    let wheel: f32 = wheel.read().map(|w| if w.unit == MouseScrollUnit::Line { w.y } else { w.y / 40.0 }).sum();
    let (Some(mut script), Some(mut sim)) = (script, sim) else { return };
    let mut ops: Vec<(Op, &str)> = Vec::new();
    // A world aid (`crate::pause::WorldAidClock`).
    let now = time.world_secs();
    // The steps wait for the scripts (a new game's take seconds to load).
    while script.rt.is_some() && aid.0.first().is_some_and(|(t, _)| now.is_some_and(|now| *t <= now)) {
        let (_, op) = aid.0.remove(0);
        info!("test aid: levelup {op:?}");
        ops.push((op, "test aid NVF_PLAY_TEST_LEVELUP"));
    }
    let live = st.open.is_some() && !st.answered && !console.open;
    let mut hover = None;
    if live {
        let page = st.open.as_ref().map_or(1, |m| m.visible() as i32);
        for (k, op) in [
            ([ArrowUp, KeyW], Op::Step(-1)),
            ([ArrowDown, KeyS], Op::Step(1)),
            ([PageUp, PageUp], Op::Step(-page)),
            ([PageDown, PageDown], Op::Step(page)),
            ([Enter, Space], Op::Enter),
            ([NumpadEnter, NumpadEnter], Op::Enter),
            ([KeyR, KeyR], Op::Reset),
            ([KeyA, KeyA], Op::Continue),
            ([KeyE, KeyE], Op::Back),
        ] {
            if keys.any_just_pressed(k) {
                ops.push((op, "key"));
            }
        }
        // Left and Right: the keyboard's, with its own repeat while held, and the
        // controller's D-pad (issue #1910).
        for right in crate::pad::sideways(&typed, &keys) {
            ops.push((if right { Op::Raise(None) } else { Op::Lower(None) }, "key"));
        }
        // The player's tweak: on the skills page the wheel raises and lowers the hovered
        // (or chosen) skill instead of scrolling [design: docs/design/tweaks.md].
        let wheel_sets = tweaks.as_ref().is_some_and(|t| t.levelup_wheel);
        let on_skills = st.open.as_ref().is_some_and(|m| m.page == model::Page::Skills);
        let hovered_part = st.open.as_ref().and_then(|m| m.hover);
        for op in levelup_wheel_ops(wheel, wheel_sets, on_skills, hovered_part) {
            ops.push((op, "wheel"));
        }
        for (i, r) in &rows {
            match i {
                PointerState::Hovered => ops.push((Op::Select(r.0), "pointer")),
                PointerState::Pressed => {
                    ops.push((Op::Select(r.0), "pointer"));
                    ops.push((Op::Click(r.0), "click"));
                }
                PointerState::None => {}
            }
        }
        for (i, p) in &parts {
            hover = Some((*p, *i != PointerState::None));
            if *i == PointerState::Pressed {
                let op = match *p {
                    Part::Reset => Op::Reset,
                    Part::Continue => Op::Continue,
                    Part::Back => Op::Back,
                    Part::ScrollUp => Op::Scroll(-1),
                    Part::ScrollDown => Op::Scroll(1),
                    Part::Plus(r) => Op::Raise(Some(r)),
                    Part::Minus(r) => Op::Lower(Some(r)),
                };
                ops.push((op, "click"));
            }
        }
    }
    let st: &mut LevelUpMenu = &mut st;
    // The pointer over a button or an arrow draws it lit.
    if let (Some((p, over)), Some(m)) = (hover, st.open.as_mut()) {
        let next = if over { Some(p) } else if m.hover == Some(p) { None } else { m.hover };
        if next != m.hover {
            if next.is_some_and(|p| matches!(p, Part::Reset | Part::Continue | Part::Back)) {
                sounds.write(UiSound { menu: UiMenu::LevelUp, event: UiEvent::Focus });
            }
            m.hover = next;
            st.dirty = true;
        }
    }
    for (op, how) in ops {
        if let Some(event) = apply(st, &mut script, &mut sim, op, how) {
            sounds.write(UiSound { menu: UiMenu::LevelUp, event });
        }
        if st.answered {
            break;
        }
    }
}

/// Mouse wheel operations on the level-up menu: when `levelup_wheel` is enabled on the
/// Skills page, wheel up raises and wheel down lowers the hovered arrow's row (or the
/// selected row); otherwise scrolls as usual [design; Stewie's Tweaks 9.65 public changelog].
fn levelup_wheel_ops(wheel: f32, levelup_wheel: bool, on_skills: bool, hover: Option<Part>) -> Vec<Op> {
    if wheel.abs() < 1.0 {
        return Vec::new();
    }
    if levelup_wheel && on_skills {
        let target = match hover {
            Some(Part::Plus(r) | Part::Minus(r)) => Some(r),
            _ => None,
        };
        let op = if wheel > 0.0 { Op::Raise(target) } else { Op::Lower(target) };
        vec![op; wheel.abs().round() as usize]
    } else {
        vec![Op::Scroll(-wheel.round() as i32)]
    }
}

/// Apply one change; the sound it makes.
fn apply(st: &mut LevelUpMenu, script: &mut ScriptState, sim: &mut SimState, op: Op, how: &str) -> Option<UiEvent> {
    match op {
        Op::Advance => {
            let level = with_game(script, sim, rules::advance)?;
            info!("levelup: advanced to level {level} ({how})");
            return None;
        }
        Op::Open => {
            with_game(script, sim, rules::open);
            return None;
        }
        _ => {}
    }
    if st.answered || st.open.is_none() {
        return None;
    }
    if op == Op::Continue {
        st.dirty = true;
        return Some(press_continue(st, script, sim, how));
    }
    let m = st.open.as_mut()?;
    let before = m.clone();
    let mut event = None;
    match op {
        Op::Named(name, then) => {
            let found = with_game(script, sim, |g| m.find(g, &name)).flatten();
            let Some(i) = found else {
                warn!("test aid: levelup: no row {name:?} on the {:?} page", m.page);
                return None;
            };
            m.select(i);
            let op = if *then == Op::Enter && m.page == Page::Perks { Op::Click(i) } else { *then };
            return apply(st, script, sim, op, how);
        }
        Op::Step(by) => m.step(by),
        Op::Select(i) => m.select(i),
        Op::Scroll(by) => m.scroll_by(by),
        Op::Raise(row) | Op::Lower(row) if m.page == Page::Skills => {
            let up = matches!(op, Op::Raise(_));
            let i = row.unwrap_or(m.selected);
            if row.is_some() {
                m.select(i);
            }
            let ok = if up { m.skills.raise(i) } else { m.skills.lower(i) };
            event = Some(if ok { UiEvent::Value { up } } else { UiEvent::Refused });
            let name = with_game(script, sim, |g| fnv_game::chargen::skill_name(g, m.skills.skills[i])).unwrap_or_default();
            if ok {
                info!("levelup: {name} {} -> {}; {} points left", before.skills.value(i), m.skills.value(i), m.skills.remaining());
            } else {
                info!("levelup: {name} {} refused", if up { "raise" } else { "lower" });
            }
        }
        Op::Raise(_) | Op::Lower(_) => {}
        Op::Enter if m.page == Page::Skills => return apply(st, script, sim, Op::Raise(None), how),
        Op::Enter => {
            let i = m.selected;
            return apply(st, script, sim, Op::Click(i), how);
        }
        Op::Click(i) => {
            if let Some(p) = m.perks.as_mut() {
                let &perk = p.perks.get(i)?;
                let ok = p.toggle(perk);
                event = Some(if ok { UiEvent::Picked } else { UiEvent::Refused });
                let taken = p.chosen.contains(&perk);
                let name = with_game(script, sim, |g| g.data.perks.get(&perk).map(|p| p.name.clone())).flatten().unwrap_or_default();
                info!("levelup: {name} {}", if !ok { "refused" } else if taken { "chosen" } else { "given back" });
            }
        }
        Op::Reset => {
            m.reset();
            info!("levelup: reset");
            event = Some(UiEvent::Reset);
        }
        Op::Back => {
            if m.page == Page::Perks {
                let skills = m.skills.clone();
                with_game(script, sim, |g| skills.undo(g));
                m.back();
                info!("levelup: back to the skills");
                event = Some(UiEvent::Switch);
            }
        }
        Op::Advance | Op::Open | Op::Continue => {}
    }
    if let Some(m) = st.open.as_ref() {
        // The chosen row moved (keys, wheel, pointer).
        if event.is_none() && m.selected != before.selected {
            event = Some(UiEvent::Focus);
        }
        st.dirty |= *m != before;
    }
    st.dirty |= st.answered;
    event
}

/// Continue / Done (A): on to the perks, or done.
fn press_continue(st: &mut LevelUpMenu, script: &mut ScriptState, sim: &mut SimState, how: &str) -> UiEvent {
    press(st, script, sim, how, false, false)
}

/// Continue / Done, or a script's page change: `forced` turns the page whatever is left
/// to allot, `done` closes the menu from any page. SetLevelUpMenuCanExitEarly 1 lets the
/// player leave with points left too (`nvse_showoff5`).
fn press(st: &mut LevelUpMenu, script: &mut ScriptState, sim: &mut SimState, how: &str, forced: bool, done: bool) -> UiEvent {
    let early = with_game(script, sim, |g| g.showoff.b5.levelup.can_exit_early).unwrap_or(false);
    let Some(m) = st.open.as_mut() else { return UiEvent::Refused };
    if !m.can_continue() && !forced && !early {
        match m.page {
            Page::Skills => info!("levelup: Continue refused: {} points left", m.skills.remaining()),
            Page::Perks => info!("levelup: Done refused: no perk chosen"),
        }
        return UiEvent::Refused;
    }
    let gains = with_game(script, sim, |g| m.skills.gains().iter().map(|&(s, d)| format!("{} +{d}", fnv_game::chargen::skill_name(g, s))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    if m.page == Page::Skills && (m.perk_due || m.preview_perks) && !done {
        let skills = m.skills.clone();
        let level = m.level;
        // The player's tweak: a preview takes no perk [design: docs/design/tweaks.md].
        let preview = !m.perk_due;
        let perks = with_game(script, sim, |g| {
            skills.apply(g);
            let mut p = rules::PerkSheet::of_player(g, level);
            if preview {
                p.count = 0;
            }
            p
        });
        let Some(perks) = perks else { return UiEvent::Refused };
        info!("levelup: skills {}; perks page: {} perks listed, {} can be taken", or_none(&gains), perks.perks.len(), perks.available.iter().filter(|&&a| a).count());
        m.to_perks(perks);
        st.dirty = true;
        return UiEvent::Switch;
    }
    let (page, skills, perks, level) = (m.page, m.skills.clone(), m.perks.clone(), m.level);
    let perk_names = with_game(script, sim, |g| perks.iter().flat_map(|p| &p.chosen).filter_map(|p| g.data.perks.get(p)).map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let done = chargen::finish(script, sim, menus::LEVEL_UP, how, |g| {
        // The skills were written on the way to the perks page.
        if page == Page::Skills {
            skills.apply(g);
        }
        if let Some(p) = &perks {
            p.apply(g);
        }
    });
    if !done {
        return UiEvent::Refused;
    }
    info!("levelup: done, level {level}: skills {}; perk {}", or_none(&gains), or_none(&perk_names));
    st.answered = true;
    UiEvent::Accepted
}

fn or_none(s: &str) -> &str {
    if s.is_empty() {
        "none"
    } else {
        s
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_menu(
    mut commands: Commands,
    mut st: ResMut<LevelUpMenu>,
    roots: Query<Entity, With<LevelUpRoot>>,
    script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<SimState>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    (windows, safe): (Query<&Window, With<PrimaryWindow>>, Option<Res<crate::touch_safe_area::SafeInsets>>),
    mut last_frame: Local<Option<(Vec2, crate::touch_safe_area::SafeInsets, f32)>>,
) {
    let view_px = windows.single().map_or(Vec2::new(1280.0, 720.0), |w| Vec2::new(w.width(), w.height()));
    let safe_insets = safe.as_deref().copied().unwrap_or_default();
    let frame_key = (view_px, safe_insets, crate::interface::unit_scale());
    if st.open.is_some() && *last_frame != Some(frame_key) {
        st.dirty = true;
    }
    if !st.dirty {
        return;
    }
    st.dirty = false;
    *last_frame = Some(frame_key);
    for e in &roots {
        commands.entity(e).despawn();
    }
    let (Some(mut script), Some(mut sim)) = (script, sim) else { return };
    if st.answered {
        return;
    }
    let Some(view) = st.open.as_ref().and_then(|m| with_game(&mut script, &mut sim, |g| m.view(g))) else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let st = st.into_inner();
    let kit = st.kit.get_or_insert_with(|| chargen::draw::Kit::load(data.as_deref(), &mut images, bc));
    let mut c = chargen::draw::Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    let fit = crate::interface::fit_centered_box(frame_key.2, view_px, safe_insets, 1088.0, 840.0);
    let mut root = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    if let Some((_, pad)) = fit {
        root.padding = pad;
    }
    let _units = fit.map(|(s, _)| crate::interface::units_at(s));
    commands.spawn((root, GlobalZIndex(crate::interface::TOP_MENU_Z), crate::interface::top_menu_backing(), LevelUpRoot)).with_children(|r| draw::spawn(r, &view, &mut c));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_aid_reads() {
        let steps = parse_aid("9:done, 2:raise:Science:3,1:advance,5:pick:Toughness,6:back,4:lower:Guns,3:reset").unwrap();
        assert_eq!(steps[0], (1.0, Op::Advance));
        assert_eq!(steps.iter().filter(|s| s.0 == 2.0).count(), 3);
        assert_eq!(steps[1].1, Op::Named("Science".into(), Box::new(Op::Raise(None))));
        assert_eq!(steps.last().unwrap().1, Op::Continue);
        assert!(parse_aid("1:jump").is_err());
        assert!(parse_aid("1:raise").is_err());
        assert!(parse_aid("1:raise:Guns:x").is_err());
    }

    #[test]
    fn only_the_level_up_menu_is_drawn_here() {
        assert!(draws(menus::LEVEL_UP));
        assert!(!draws(menus::TAG_SKILLS) && !chargen::has_own_menu(menus::LEVEL_UP));
    }

    #[test]
    fn levelup_wheel_adjusts_hovered_arrow_row_on_skills_page() {
        // Off by default: scrolls even on the Skills page.
        assert_eq!(levelup_wheel_ops(1.0, false, true, Some(Part::Plus(3))), vec![Op::Scroll(-1)]);
        // Enabled on Skills: wheel up raises, wheel down lowers, targeting hovered +/- row.
        assert_eq!(levelup_wheel_ops(2.0, true, true, Some(Part::Plus(3))), vec![Op::Raise(Some(3)), Op::Raise(Some(3))]);
        assert_eq!(levelup_wheel_ops(-1.0, true, true, Some(Part::Minus(4))), vec![Op::Lower(Some(4))]);
        assert_eq!(levelup_wheel_ops(1.0, true, true, None), vec![Op::Raise(None)]);
        // Perks page always scrolls.
        assert_eq!(levelup_wheel_ops(-1.0, true, false, None), vec![Op::Scroll(1)]);
    }
}
