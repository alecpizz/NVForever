//! [design, #2403] Read-only controls-lock evidence for reports, registered only when
//! reports are enabled (`reports/mod.rs`). Runtime fields: `fnv_game::game::Game`;
//! engine screen state: `nvf_play::{menu,pause,pipboy,wait}`.

use super::anomaly::{self, LockProgress};
use super::Collector;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseButtonInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use fnv_game::game::Game;
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};

#[derive(SystemParam)]
pub(super) struct Presses<'w, 's> {
    keyboard: MessageReader<'w, 's, KeyboardInput>,
    mouse: MessageReader<'w, 's, MouseButtonInput>,
    pads: Query<'w, 's, &'static Gamepad>,
}

impl Presses<'_, '_> {
    fn count(&mut self) -> usize {
        // [design / file refs] Raw keyboard/mouse messages survive UI consumption;
        // pad.rs's synthesized menu keys are not messages and must not count again.
        self.keyboard.read().filter(key_press).count()
            + self.mouse.read().filter(|e| e.state == ButtonState::Pressed).count()
            + self.pads.iter().map(|p| p.get_just_pressed().count()).sum::<usize>()
    }
}

fn key_press(e: &&KeyboardInput) -> bool {
    e.state == ButtonState::Pressed && !e.repeat
}

#[derive(SystemParam)]
pub(super) struct Menus<'w> {
    state: Option<Res<'w, State<nvf_play::PlayState>>>,
    paused: Option<Res<'w, nvf_play::pause::Paused>>,
    page: Option<Res<'w, nvf_play::pause::Page>>,
    pause_view: Option<Res<'w, nvf_play::pause::PauseView>>,
    settings_tab: Option<Res<'w, nvf_play::pause::SettingsTabStore>>,
    pipboy: Option<Res<'w, nvf_play::pipboy::PipBoy>>,
    data: Option<Res<'w, nvf_play::pipboy::data::DataState>>,
    stats: Option<Res<'w, nvf_play::pipboy::stats::StatsView>>,
    stats_snapshot: Option<Res<'w, nvf_play::pipboy::stats::Prepared>>,
    holotape: Option<Res<'w, nvf_play::holotape::Holotape>>,
    repair: Option<Res<'w, nvf_play::pipboy::repair::PipRepair>>,
    item_mod: Option<Res<'w, nvf_play::pipboy::item_mod::PipItemMod>>,
    active: Option<Res<'w, nvf_play::menu::ActiveMenu>>,
    wait: Option<Res<'w, nvf_play::wait::WaitMenu>>,
    terminal: Option<Res<'w, nvf_play::terminal::TerminalScreen>>,
    levelup: Option<Res<'w, nvf_play::levelup::LevelUpMenu>>,
    dialogue: Option<Res<'w, nvf_play::dialogue::DialogueScreen>>,
    builtins: Builtins<'w>,
    games: Games<'w>,
}

/// [design, #2403 / file refs] Other holds audited against `script::hold_player`
/// and `script::is_script_menu`; nested params keep each family read-only.
#[derive(SystemParam)]
struct Builtins<'w> {
    recipe: Option<Res<'w, nvf_play::crafting::RecipeMenu>>,
    lockpick: Option<Res<'w, nvf_play::lockpick::LockpickScreen>>,
    repair: Option<Res<'w, nvf_play::repair::RepairServices>>,
    message: Option<Res<'w, nvf_play::message_menu::MessagePanel>>,
    console: Option<Res<'w, nvf_play::console::Console>>,
    wheel: Option<Res<'w, nvf_play::companion_wheel::WheelScreen>>,
    hud: Option<Res<'w, nvf_play::script_hud::ScriptHud>>,
    chargen: Option<Res<'w, nvf_play::chargen::ChargenMenus>>,
    creator: Option<Res<'w, nvf_play::creator::CreatorMenu>>,
    tutorial: Option<Res<'w, nvf_play::tutorial_menu::TutorialView>>,
    quantity: Option<Res<'w, nvf_play::script_quantity::ScriptQuantity>>,
    vigor: Option<Res<'w, nvf_play::chargen::vigor::VigorMenu>>,
    hotkey: Option<Res<'w, nvf_play::pipboy::hotkey_panel::HotkeyHold>>,
    hotkeys: Option<Res<'w, nvf_play::hotkeys::Hotkeys>>,
}

#[derive(SystemParam)]
struct Games<'w> {
    blackjack: Option<Res<'w, nvf_play::blackjack::BlackjackScreen>>,
    roulette: Option<Res<'w, nvf_play::roulette::RouletteScreen>>,
    slots: Option<Res<'w, nvf_play::slots::SlotsScreen>>,
    poker: Option<Res<'w, nvf_play::poker::PokerScreen>>,
    caravan: Option<Res<'w, nvf_play::caravan::CaravanScreen>>,
}

fn hash_debug(value: &impl std::fmt::Debug, hash: &mut impl Hasher) {
    format!("{value:?}").hash(hash);
}

fn cut(label: &str) -> String {
    label.chars().take(anomaly::CONTROLS_LOCKED_LABEL_CHARS).collect()
}

fn menu_name(code: u16) -> String {
    // [design / file refs] Diagnostic labels for the existing `game::menus` codes.
    use fnv_game::game::menus;
    match code {
        menus::MESSAGE => "MessageMenu".into(),
        menus::INVENTORY => "InventoryMenu".into(),
        menus::STATS => "StatsMenu".into(),
        menus::START => "StartMenu".into(),
        menus::MAP => "MapMenu".into(),
        menus::SLEEP_WAIT => "SleepWaitMenu".into(),
        menus::BARTER => "BarterMenu".into(),
        menus::DIALOGUE => "DialogMenu".into(),
        menus::LOCKPICK => "LockPickMenu".into(),
        menus::REPAIR => "RepairMenu".into(),
        menus::ITEM_MOD => "ItemModMenu".into(),
        menus::LEVEL_UP => "LevelUpMenu".into(),
        menus::TUTORIAL => "TutorialMenu".into(),
        menus::QUANTITY => "QuantityMenu".into(),
        menus::COMPANION_WHEEL => "CompanionWheel".into(),
        nvf_play::script::MENU_CONTAINER => "ContainerMenu".into(),
        _ => nvf_play::script_hud::menu_name(code),
    }
}

fn groups(flags: [bool; 7]) -> Vec<&'static str> {
    flags.into_iter().zip(nvf_play::control::group::NAMES).filter_map(|(locked, name)| locked.then_some(name)).collect()
}

/// [design / file refs] Standard control flags keep the last DisablePlayerControls call
/// that holds a group (`PlayerState::controls_disabled_by`: its script unit, quest and
/// reference, #3044); Alt flags retain plugin names (`Game::other.alt_locks`).
fn sources(standard: [bool; 7], caller: Option<&fnv_game::game::ControlsCaller>, alt: &std::collections::BTreeMap<String, [bool; 7]>) -> (Vec<Value>, usize) {
    let standard = standard.iter().any(|x| *x).then(|| {
        json!({
            "by": "DisablePlayerControls",
            "script": caller.map(|c| cut(&c.unit)),
            "quest": caller.and_then(|c| c.quest).map(|q| format!("{:08X}", q.0)),
            "ref": caller.and_then(|c| c.reference).map(|r| format!("{:08X}", r.0)),
            "groups": groups(standard),
        })
    });
    let all = standard.into_iter().chain(
        alt.iter()
            .filter(|(_, flags)| flags.iter().any(|x| *x))
            .map(|(plugin, flags)| json!({ "by": "DisablePlayerControlsAlt", "plugin": cut(plugin), "groups": groups(*flags) })),
    );
    let mut kept = Vec::new();
    let mut omitted = 0;
    for source in all {
        if kept.len() < anomaly::CONTROLS_LOCKED_SOURCES {
            kept.push(source);
        } else {
            omitted += 1;
        }
    }
    (kept, omitted)
}

/// [design, #2403, #3044] The quests that tell which scene holds the player: the lock's
/// own quest (`owner`) and those whose stage changed since the hold began (`at_start`),
/// running or not, in FormID order, bounded. Not every running quest: the list said
/// nothing and made the row large.
fn quest_stages<'a>(
    quests: impl Iterator<Item = (&'a fnv_sim::FormId, &'a fnv_game::quest::QuestState)>,
    owner: Option<fnv_sim::FormId>,
    at_start: &std::collections::HashMap<fnv_sim::FormId, i32>,
) -> Vec<Value> {
    quests
        .filter(|(id, q)| Some(**id) == owner || at_start.get(*id).is_none_or(|s| *s != q.stage()))
        .take(anomaly::CONTROLS_LOCKED_QUESTS)
        .map(|(id, q)| json!({ "quest": format!("{:08X}", id.0), "stage": q.stage(), "owns_lock": Some(*id) == owner }))
        .collect()
}

/// [design, #2403] Fingerprint logical progress, excluding hover, countdowns and
/// animation clocks. IDs/choice state come from `fnv_game::dialogue::Conversation`.
fn runtime_screen(g: &Game, hash: &mut impl Hasher) {
    g.menu.hash(hash);
    g.ui_prompt.hash(hash);
    g.front_box().map(|b| b.serial).hash(hash);
    dialogue_screen(g.dialogue_serial, g.dialogue.as_ref(), hash);
}

fn dialogue_screen(serial: u32, conversation: Option<&fnv_game::dialogue::Conversation>, hash: &mut impl Hasher) {
    serial.hash(hash);
    if let Some(c) = conversation {
        c.actor.hash(hash);
        c.lines.hash(hash);
        c.line.as_ref().map(|l| (l.topic, l.info, l.response)).hash(hash);
        c.topic_list.hash(hash);
        for choice in &c.choices {
            hash_debug(choice, hash);
        }
    }
}

impl Menus<'_> {
    fn screen(&self, g: Option<&Game>, hash: &mut impl Hasher) -> Option<String> {
        let mut name = None;
        if let Some(model) = self.active.as_ref().and_then(|a| a.0.as_ref()) {
            hash_debug(&(model.mode, model.focus, model.selected, model.scroll, model.category, model.quantity, model.folder), hash);
            hash_debug(&(&model.status, model.trade.as_ref().map(|t| (&t.offered, t.purse, t.balance)), model.confirming_take_all()), hash);
            for (list, selected) in model.lists.iter().zip(model.selected) {
                // [design / file refs] Counts and selection change on transfers even
                // when the menu's page stays open (`menu::model::Listing`).
                hash_debug(&(list.rows.len(), list.value), hash);
                hash_debug(&list.rows.get(selected).map(|r| (r.item, r.count, r.equipped, r.mods, r.condition)), hash);
            }
            name = Some(menu_name(nvf_play::script::menu_code(model.mode)));
        }
        if let Some(pip) = self.pipboy.as_ref().filter(|p| p.open()) {
            hash_debug(&(pip.page, pip.tabs), hash);
            if let Some(data) = self.data.as_ref().filter(|_| pip.page == nvf_play::pipboy::Page::Data) {
                hash_debug(&data.progress(pip.tabs[pip.page.index()]), hash);
            }
            if let Some(stats) = self.stats.as_ref().filter(|_| pip.page == nvf_play::pipboy::Page::Stats) {
                hash_debug(&(stats.status, stats.selected, stats.top, stats.desc, stats.reputation, stats.limb), hash);
                if let Some(s) = self.stats_snapshot.as_ref().and_then(|p| p.snap.as_ref()) {
                    hash_debug(&(s.hp, s.ap, s.xp, s.limbs, s.rads, s.needs), hash);
                }
            }
            if let Some(repair) = self.repair.as_ref().and_then(|r| r.0.as_ref()) {
                hash_debug(&repair, hash);
                name = Some("RepairMenu".into());
            } else if let Some(item_mod) = self.item_mod.as_ref().and_then(|m| m.0.as_ref()) {
                hash_debug(&item_mod, hash);
                name = Some("ItemModMenu".into());
            } else {
                name = Some(format!("Pip-Boy {:?}", pip.page));
            }
            if pip.page == nvf_play::pipboy::Page::Items {
                if let Some(hold) = &self.builtins.hotkey {
                    hash_debug(&(hold.slot, &hold.said), hash);
                }
                if let Some(keys) = &self.builtins.hotkeys {
                    hash_debug(&keys.0, hash);
                }
            }
            if pip.page == nvf_play::pipboy::Page::Data {
                if let Some(tape) = &self.holotape {
                    hash_debug(&(tape.playing, tape.paused), hash);
                }
                if let Some(g) = g {
                    hash_debug(&(g.radio.pipboy.on, g.radio.pipboy.station), hash);
                }
            }
        }
        if let Some(wait) = self.wait.as_ref().and_then(|w| w.0.as_ref()) {
            hash_debug(&(wait.sleep, wait.hours, wait.passing.map(|p| (p.asked, p.left))), hash);
            name = Some("SleepWaitMenu".into());
        }
        if let Some(terminal) = self.terminal.as_ref().and_then(|t| t.0.as_ref()) {
            use nvf_play::terminal::Page;
            terminal.reference.hash(hash);
            terminal.base.hash(hash);
            // [design, #2403 / file refs] Fingerprint the terminal.rs page and its
            // visible navigation/result state, excluding hover-sound and end timers.
            match &terminal.page {
                Page::Hacking(h) => {
                    0_u8.hash(hash);
                    hash_debug(&(h.cursor, &h.puzzle.chars, h.puzzle.attempts, &h.puzzle.log, h.puzzle.state), hash);
                    name = Some("HackingMenu".into());
                }
                Page::LockedOut(lines) => {
                    1_u8.hash(hash);
                    lines.hash(hash);
                    name = Some("HackingMenu".into());
                }
                Page::Menu(m) => {
                    2_u8.hash(hash);
                    hash_debug(&(m.terminal, &m.parents, &m.items, &m.labels, m.selected, &m.result, m.note, m.scroll), hash);
                    name = Some("TerminalMenu".into());
                }
            }
        }
        if let Some(progress) = self.levelup.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            name = Some("LevelUpMenu".into());
        }
        self.games.screen(hash, &mut name);
        self.builtins.screen(g, hash, &mut name);
        if g.is_some_and(|g| g.dialogue.is_some()) || self.state.as_ref().is_some_and(|s| *s.get() == nvf_play::PlayState::Dialogue) {
            if let Some(dialogue) = &self.dialogue {
                hash_debug(&dialogue.progress(), hash);
            }
        }
        if self.paused.as_ref().is_some_and(|p| p.0) {
            hash_debug(&self.page.as_deref(), hash);
            if let Some(view) = &self.pause_view {
                hash_debug(&view.progress(), hash);
            }
            hash_debug(&self.settings_tab.as_ref().and_then(|t| t.tab.as_ref()), hash);
            name = Some("StartMenu".into());
        }
        if name.is_none() {
            name = g.and_then(|g| g.menu).map(menu_name);
        }
        if name.is_none() {
            name = match self.state.as_ref().map(|s| s.get()) {
                Some(nvf_play::PlayState::Menu) => Some("engine menu".into()),
                Some(nvf_play::PlayState::Dialogue) => Some("DialogMenu".into()),
                _ => None,
            };
        }
        name.hash(hash);
        name
    }
}

impl Builtins<'_> {
    fn screen(&self, g: Option<&Game>, hash: &mut impl Hasher, name: &mut Option<String>) {
        if let Some(progress) = self.recipe.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            *name = Some("RecipeMenu".into());
        }
        if let Some(s) = self.lockpick.as_ref().and_then(|m| m.0.as_ref()) {
            // [design, #2403 / file refs: lockpick.rs] Phase identity and actual
            // pin/cylinder movement count; PinBroken::since and fade clocks do not.
            hash_debug(&(s.reference, s.game.pin, s.game.turn, s.game.pins, std::mem::discriminant(&s.game.phase),
                &s.status, &s.help, s.leaving.is_some()), hash);
            *name = Some("LockPickMenu".into());
        }
        if let Some(progress) = self.repair.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            *name = Some("RepairServicesMenu".into());
        }
        if let Some(panel) = self.message.as_ref().filter(|p| p.view.is_some()) {
            hash_debug(&(&panel.view, panel.showing, panel.selected, panel.chosen), hash);
            *name = Some("MessageMenu".into());
        }
        if let Some(progress) = self.console.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            *name = Some("Console".into());
        }
        if let Some(s) = self.wheel.as_ref().and_then(|m| m.0.as_ref()) {
            hash_debug(&(s.companion, s.selected, &s.buttons), hash);
            if let Some(hud) = &self.hud {
                hud.wheel_line.hash(hash);
            }
            *name = Some("CompanionWheel".into());
        }
        if let Some((code, progress)) = self.chargen.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            *name = Some(menu_name(code));
        }
        if let Some(progress) = self.creator.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            *name = Some("RaceMenu".into());
        }
        if let Some(progress) = self.quantity.as_ref().and_then(|m| m.progress()) {
            hash_debug(&progress, hash);
            *name = Some("QuantityMenu".into());
        }
        if let Some(progress) = self.vigor.as_ref().and_then(|m| m.progress()) {
            hash_debug(progress, hash);
            *name = Some("VigorTesterMenu".into());
        }
        if let Some(g) = g.filter(|g| g.menu == Some(fnv_game::game::menus::TUTORIAL)) {
            g.tutorial.hash(hash);
            if let Some(t) = &self.tutorial {
                hash_debug(&(t.page, t.scroll), hash);
            }
        }
    }
}

impl Games<'_> {
    fn screen(&self, hash: &mut impl Hasher, name: &mut Option<String>) {
        // [design, #2403 / file refs: nvf-play/src/{blackjack,roulette,slots,poker,caravan}.rs]
        // Keep game outcomes and choices; exclude scheduled sounds, RNG, animation
        // timestamps and attempted-input counters from these local-only fingerprints.
        if let Some(s) = self.blackjack.as_ref().and_then(|m| m.0.as_ref()) {
            let t = &s.table;
            hash_debug(&(s.casino, s.bet, s.chips, s.earnings, &s.status, &s.help), hash);
            hash_debug(&(t.phase, &t.hands, t.active, &t.dealer, t.hole_shown, t.rounds, s.leaving.is_some()), hash);
            *name = Some("BlackjackMenu".into());
        }
        if let Some(s) = self.roulette.as_ref().and_then(|m| m.0.as_ref()) {
            hash_debug(&(s.casino, s.bet, &s.cursor, &s.table, std::mem::discriminant(&s.phase),
                s.chips, s.earnings, &s.status, &s.help, s.last, s.banned), hash);
            *name = Some("RouletteMenu".into());
        }
        if let Some(s) = self.slots.as_ref().and_then(|m| m.0.as_ref()) {
            hash_debug(&(s.casino, s.bet, s.stops, std::mem::discriminant(&s.phase), s.chips,
                s.earnings, &s.status, &s.help, s.payouts, s.banned), hash);
            *name = Some("SlotsMenu".into());
        }
        if let Some(s) = self.poker.as_ref().and_then(|m| m.0.as_ref()) {
            let t = &s.table;
            hash_debug(&(s.casino, s.raise_to, &s.status, s.ended.as_ref().map(|(_, why)| why), s.leaving.is_some()), hash);
            hash_debug(&(&t.seats, t.button, &t.community, t.street, t.to_act, t.current_bet, t.hand, &t.winners, t.showdown), hash);
            *name = Some("PokerMenu".into());
        }
        if let Some(s) = self.caravan.as_ref().and_then(|m| m.0.as_ref()) {
            hash_debug(&(s.opponent, s.stage, &s.bet, &s.deck, s.cursor, s.hand, &s.pick, s.forfeit_armed), hash);
            hash_debug(&(&s.status, &s.help, s.game.as_ref().map(|g| (&g.sides, g.turn, g.phase, &g.winner)), s.leaving.is_some()), hash);
            *name = Some("CaravanMenu".into());
        }
    }
}

pub(super) fn detect(
    real: Res<Time<Real>>,
    loading: Option<Res<nvf_play::loading::Loading>>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    menus: Menus,
    mut presses: Presses,
    mut c: ResMut<Collector>,
    mut stages_at_start: Local<Option<std::collections::HashMap<fnv_sim::FormId, i32>>>,
) {
    let presses = presses.count();
    let now = real.elapsed_secs_f64();
    let g = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let menu = menus.screen(g, &mut hash);
    let dialogue = g.and_then(|g| g.dialogue.as_ref());
    let flags = g.map_or([false; 7], |g| g.controls_locked());
    if let Some(g) = g {
        runtime_screen(g, &mut hash);
    }
    // The pause menu is the player's own and can always be left: nothing counts as held
    // while it is up (#3044: most 0.8.0 events were a player in the pause menu).
    let paused = menus.paused.as_ref().is_some_and(|p| p.0);
    let held = !paused && (menu.is_some() || dialogue.is_some() || flags.iter().any(|x| *x));
    let progress = (held && !loading.as_ref().is_some_and(|l| l.active())).then_some(LockProgress { screen: hash.finish(), groups: flags });
    // The quests' stages as a hold begins, to name those that move during it.
    match (&progress, g) {
        (None, _) => *stages_at_start = None,
        (Some(_), Some(g)) if stages_at_start.is_none() => *stages_at_start = Some(g.quests.iter().map(|(id, q)| (*id, q.stage())).collect()),
        _ => {}
    }
    let c = &mut *c;
    let Some(held_s) = c.detectors.controls_locked.step(now, progress, presses) else { return };
    let (sources, omitted) = g.map_or_else(|| (Vec::new(), 0), |g| sources(g.player.controls_disabled, g.player.controls_disabled_by.as_ref(), &g.other.alt_locks));
    let dialogue = dialogue.map(|d| {
        json!({
            "speaker": format!("{:08X}", d.actor.0),
            "speaker_name": g.map(|g| cut(&g.data.name(d.actor))),
            "topic": d.line.as_ref().map(|l| format!("{:08X}", l.topic.0)),
            "info": d.line.as_ref().map(|l| format!("{:08X}", l.info.0)),
            "response": d.line.as_ref().map(|l| l.response),
            "offered_topics": d.choices.iter().take(anomaly::CONTROLS_LOCKED_TOPICS).map(|c| format!("{:08X}", c.topic.0)).collect::<Vec<_>>(),
        })
    });
    let owner = g.and_then(|g| g.player.controls_disabled_by.as_ref()).and_then(|c| c.quest);
    let none = std::collections::HashMap::new();
    let quests = g.map(|g| quest_stages(g.quests.iter(), owner, stages_at_start.as_ref().unwrap_or(&none))).unwrap_or_default();
    c.detectors.fire(
        &mut c.summary,
        &c.here,
        c.started.elapsed().as_secs(),
        "controls_locked",
        json!({
            "held_s": held_s.round(), "recent_presses": anomaly::CONTROLS_LOCKED_PRESSES,
            "menu": menu, "dialogue": dialogue, "locked_groups": groups(flags),
            "lock_sources": sources, "lock_sources_omitted": omitted, "quests": quests,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::Key;

    fn detector_app() -> App {
        let now = std::time::Instant::now();
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .insert_resource(State::new(nvf_play::PlayState::Menu))
            .add_message::<KeyboardInput>()
            .add_message::<MouseButtonInput>()
            .insert_resource(Collector {
                summary: Default::default(),
                file: Default::default(),
                started: now,
                last_write: now,
                places: Default::default(),
                cells_seen: Default::default(),
                last_space: None,
                space_time: Default::default(),
                start_kind: "test",
                load: None,
                here: Default::default(),
                load_ended: 0.0,
                away_until: 0.0,
                detectors: Default::default(),
            })
            .add_systems(Update, detect);
        app
    }

    fn sample(app: &mut App, now: f64, presses: usize) {
        let mut time = app.world_mut().resource_mut::<Time<Real>>();
        let delta = now - time.elapsed_secs_f64();
        time.advance_by(std::time::Duration::from_secs_f64(delta));
        for _ in 0..presses {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyE,
                logical_key: Key::Character("e".into()),
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
    }

    /// [design, #2403] Run the actual collection/detection system, with no reports
    /// plugin, writers, network or game data. Each pair isolates one progress signal.
    fn navigation_resets_detection<R: Resource>(before: R, after: R, ineffective: R, setup: impl FnOnce(&mut World), menu: &str) {
        let mut app = detector_app();
        setup(app.world_mut());
        app.insert_resource(before);
        sample(&mut app, 0.0, 0);
        sample(&mut app, 59.0, 5);
        assert!(app.world().resource::<Collector>().summary.events.is_empty());
        app.insert_resource(after);
        sample(&mut app, 60.0, 5);
        assert!(app.world().resource::<Collector>().summary.events.is_empty(), "navigation at the old deadline resets the watch");
        sample(&mut app, 119.0, 5);
        assert!(app.world().resource::<Collector>().summary.events.is_empty(), "59 seconds since progress");
        app.insert_resource(ineffective);
        sample(&mut app, 120.0, 5);
        let events = &app.world().resource::<Collector>().summary.events;
        assert_eq!(events.len(), 1, "ineffective input must not reset the timer");
        assert_eq!(events[0]["kind"], "controls_locked");
        assert_eq!(events[0]["menu"], menu);
        assert_eq!(events[0]["held_s"], 60.0);
        sample(&mut app, 180.0, 5);
        assert_eq!(app.world().resource::<Collector>().summary.events.len(), 1, "once per unchanged lock");
    }

    #[test]
    fn detect_every_hold_player_screen_resets_for_progress_but_not_attempt_counters() {
        use nvf_play::{blackjack, caravan, companion_wheel, console, crafting, creator, lockpick, poker, repair, roulette, slots};
        macro_rules! check {
            ($module:ident, $last:expr, $name:expr) => {
                for step in 1..=$last {
                    navigation_resets_detection($module::report_progress_fixture(step - 1), $module::report_progress_fixture(step),
                        $module::report_progress_fixture(if step == $last { step + 1 } else { step }), |_| {}, $name);
                }
                let mut app = detector_app();
                app.insert_resource($module::report_progress_fixture(0));
                sample(&mut app, 0.0, 0);
                sample(&mut app, 120.0, 0);
                assert!(app.world().resource::<Collector>().summary.events.is_empty(), "idle {}", $name);
            };
        }
        check!(crafting, 6, "RecipeMenu");
        check!(lockpick, 7, "LockPickMenu");
        check!(repair, 4, "RepairServicesMenu");
        check!(console, 5, "Console");
        check!(companion_wheel, 1, "CompanionWheel");
        check!(blackjack, 3, "BlackjackMenu");
        check!(roulette, 4, "RouletteMenu");
        check!(slots, 4, "SlotsMenu");
        check!(poker, 3, "PokerMenu");
        check!(caravan, 4, "CaravanMenu");
        check!(creator, 3, "RaceMenu");
    }

    #[test]
    fn detect_script_character_menus_track_edits_and_allocations_not_redraws() {
        use fnv_game::game::menus;
        use nvf_play::chargen::{report_list_progress_fixture as list, report_progress_fixture as name};
        navigation_resets_detection(name(0), name(1), name(2), |_| {}, "Name");
        for code in [menus::TRAITS, menus::TAG_SKILLS] {
            for step in 1..=3 {
                navigation_resets_detection(list(code, step - 1), list(code, step), list(code, if step == 3 { 4 } else { step }), |_| {}, &menu_name(code));
            }
        }
        use nvf_play::chargen::report_special_progress_fixture as special;
        for step in 1..=3 {
            navigation_resets_detection(special(step - 1), special(step), special(if step == 3 { 4 } else { step }), |_| {}, &menu_name(menus::TAG_SKILLS));
        }
    }

    #[test]
    fn detect_script_quantity_and_vigor_progress_but_not_redraw_or_animation_bookkeeping() {
        use nvf_play::{script_quantity::ScriptQuantity, chargen::vigor::VigorMenu};
        navigation_resets_detection(ScriptQuantity::report_progress_fixture(0), ScriptQuantity::report_progress_fixture(1), ScriptQuantity::report_progress_fixture(2), |_| {}, "QuantityMenu");
        for step in 1..=3 {
            navigation_resets_detection(VigorMenu::report_progress_fixture(step - 1), VigorMenu::report_progress_fixture(step), VigorMenu::report_progress_fixture(if step == 3 { 4 } else { step }), |_| {}, "VigorTesterMenu");
        }
    }

    fn pip_page(w: &mut World, page: nvf_play::pipboy::Page, tab: usize) {
        let mut pip = nvf_play::pipboy::PipBoy::default();
        pip.phase = nvf_play::pipboy::Phase::Up;
        pip.page = page;
        pip.tabs[page.index()] = tab;
        w.insert_resource(pip);
    }

    #[test]
    fn detect_map_pan_zoom_cursor_questions_and_active_quest_reset() {
        use nvf_play::pipboy::{data::report_map_progress_fixture as fixture, Page};
        for tab in [0, 1] {
            for step in 1..=7 {
                // [design, #2403] Both map tabs use the same state contract.
                let make = |n| {
                    let mut f = fixture(n);
                    if tab == 0 { f = nvf_play::pipboy::data::report_local_map_progress_fixture(n); }
                    f
                };
                navigation_resets_detection(make(step - 1), make(step), make(if step == 7 { 8 } else { step }),
                    |w| pip_page(w, Page::Data, tab), "Pip-Boy Data");
            }
        }
    }

    #[test]
    fn detect_inventory_barter_container_navigation_quantity_transfers_and_confirmations() {
        use nvf_play::menu::{model::{report_progress_fixture as fixture, Mode}, ActiveMenu};
        let trade = |step| ActiveMenu(Some(nvf_play::menu::model::report_trade_progress_fixture(step)));
        navigation_resets_detection(trade(0), trade(1), trade(2), |_| {}, "BarterMenu");
        for mode in [Mode::Inventory, Mode::Barter(fnv_sim::FormId(42)), Mode::Container(fnv_sim::FormId(42))] {
            for step in 1..=8 {
                let make = |n| ActiveMenu(Some(fixture(mode, n)));
                navigation_resets_detection(make(step - 1), make(step), make(if step == 8 { 9 } else { step }), |_| {}, &menu_name(nvf_play::script::menu_code(mode)));
            }
        }
    }

    #[test]
    fn detect_message_selection_and_replacement_reset_but_boundary_presses_do_not() {
        use nvf_play::message_menu::{MessagePanel, PanelView};
        let fixture = |step| {
            let mut panel = MessagePanel::default();
            panel.show(Some(PanelView { text: "Synthetic question".into(), buttons: vec!["First".into(), "Second".into()], ..Default::default() }), Some(if step >= 3 { 2 } else { 1 }));
            if step >= 1 { panel.chosen = true; }
            if step >= 2 { panel.step(1); }
            if step >= 4 { panel.step(1); }
            panel
        };
        for step in 1..=3 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 3 { 4 } else { step }), |_| {}, "MessageMenu");
        }
    }

    #[test]
    fn detect_wait_hours_and_completed_hours_reset_but_partial_hour_clock_does_not() {
        use nvf_play::wait::{model::{Command, Model}, WaitMenu};
        for sleep in [false, true] {
            let fixture = |step| {
                let mut m = Model::open(sleep);
                if step >= 1 { m.apply(Command::Hours(3)); }
                if step >= 2 { m.apply(Command::Accept); }
                if step >= 3 { m.tick(1.0, 1.0); }
                if step >= 4 { m.tick(0.1, 1.0); }
                WaitMenu(Some(m))
            };
            for step in 1..=3 {
                navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 3 { 4 } else { step }), |_| {}, "SleepWaitMenu");
            }
        }
    }

    #[test]
    fn detect_tutorial_page_and_scroll_reset_but_hover_does_not() {
        use nvf_play::tutorial_menu::TutorialView;
        let fixture = |step| TutorialView::report_progress_fixture(step);
        for step in 1..=2 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 2 { 3 } else { step }),
                |w| { w.insert_resource(nvf_play::script::report_progress_fixture(fnv_game::game::menus::TUTORIAL)); }, "TutorialMenu");
        }
    }

    #[test]
    fn detect_stats_navigation_and_visible_health_reset_but_hover_does_not() {
        use nvf_play::pipboy::{stats::{Prepared, StatsView}, Page};
        let fixture = |step| {
            let mut s = StatsView::default();
            if step >= 1 { s.status = 1; }
            if step >= 2 { s.selected[1] = 1; }
            if step >= 3 { s.top[1] = 1; }
            if step >= 4 { s.desc = 1; }
            if step >= 5 { s.reputation = false; }
            if step >= 6 { s.choose_limb(0); }
            s.highlight = step >= 7;
            s
        };
        for step in 1..=6 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 6 { 7 } else { step }),
                |w| pip_page(w, Page::Stats, 0), "Pip-Boy Stats");
        }
        let fixture = |hp, redraw| Prepared { snap: Some(nvf_play::pipboy::stats::model::Snapshot { hp: Some((hp, 100.0)), ..Default::default() }), new: redraw };
        navigation_resets_detection(fixture(50.0, false), fixture(60.0, false), fixture(60.0, true), |w| {
            pip_page(w, Page::Stats, 0);
            w.insert_resource(StatsView::default());
        }, "Pip-Boy Stats");
    }

    #[test]
    fn detect_repair_and_item_mod_navigation_and_outcomes_reset() {
        use nvf_play::pipboy::{item_mod, repair, Page};
        let fixture = |step| repair::PipRepair(Some(repair::Session {
            offer: repair::Offer { item: fnv_sim::FormId(42), extra: Default::default(), name: "Synthetic item".into(),
                gear: fnv_game::repair::Gear::Weapon, condition: if step >= 3 { 0.9 } else { 0.5 }, icon: None, parts: vec![] },
            skill: 50.0, settings: Default::default(), first: usize::from(step >= 1), selected: usize::from(step >= 2), preview: false,
        }));
        for step in 1..=3 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(step), |w| pip_page(w, Page::Items, 0), "RepairMenu");
        }
        let fixture = |step| item_mod::PipItemMod(Some(item_mod::Session {
            offer: item_mod::Offer { item: fnv_sim::FormId(42), extra: fnv_sim::inventory::Extra { mods: if step >= 3 { 1 } else { 0 }, ..Default::default() },
                name: "Synthetic item".into(), icon: None, choices: vec![] },
            first: usize::from(step >= 1), selected: usize::from(step >= 2),
        }));
        for step in 1..=3 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(step), |w| pip_page(w, Page::Items, 0), "ItemModMenu");
        }
    }

    #[test]
    fn detect_data_audio_pause_and_radio_tuning_reset_but_playback_counters_do_not() {
        use nvf_play::{holotape::Holotape, pipboy::Page};
        let fixture = |step| {
            let mut tape = Holotape::default();
            if step >= 1 { tape.playing = Some(fnv_sim::FormId(42)); }
            if step >= 2 { tape.paused = true; }
            tape.started = step as u64;
            tape
        };
        for step in 1..=2 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 2 { 3 } else { step }), |w| pip_page(w, Page::Data, 3), "Pip-Boy Data");
        }
        let fixture = |step| {
            let mut s = nvf_play::script::report_progress_fixture(fnv_game::game::menus::MAP);
            let g = &mut s.rt.as_mut().unwrap().game;
            g.radio.pipboy.station = Some(fnv_sim::FormId(if step >= 2 { 43 } else { 42 }));
            g.radio.pipboy.on = step >= 1;
            g.frame = step as u64;
            s
        };
        for step in 1..=2 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 2 { 3 } else { step }), |w| pip_page(w, Page::Data, 4), "Pip-Boy Data");
        }
    }

    #[test]
    fn detect_hotkey_assignment_and_feedback_reset_but_aid_bookkeeping_does_not() {
        use nvf_play::{hotkeys::{Hotkeys, Slot}, pipboy::{hotkey_panel::HotkeyHold, Page}};
        let mut keys = Hotkeys::default();
        let before = keys.clone();
        keys.0[0] = Some(Slot { item: 42, mods: 0 });
        navigation_resets_detection(before, keys.clone(), keys, |w| pip_page(w, Page::Items, 0), "Pip-Boy Items");
        let fixture = |step| HotkeyHold { slot: Some(0), said: (step >= 1).then(|| "Synthetic assignment".into()), aid: (step >= 2).then_some(0), controller: None };
        navigation_resets_detection(fixture(0), fixture(1), fixture(2), |w| pip_page(w, Page::Items, 0), "Pip-Boy Items");
    }

    fn terminal_fixture(step: usize) -> nvf_play::terminal::TerminalScreen {
        use fnv_sim::FormId;
        use nvf_play::terminal::{MenuView, Page, Session, TerminalScreen};
        let mut m = MenuView {
            terminal: FormId(42),
            parents: Vec::new(),
            items: vec![0, 1],
            labels: vec!["Synthetic first row".into(), "Synthetic second row".into()],
            selected: 0,
            hidden: false,
            result: String::new(),
            note: None,
            scroll: 0,
            server: 0,
            welcome: String::new(),
        };
        if step >= 1 {
            m.selected = 1;
        }
        if step >= 2 {
            m.parents.push((m.terminal, m.selected));
            m.terminal = FormId(43);
        }
        if step >= 3 {
            m.note = Some(FormId(44));
        }
        if step >= 4 {
            m.scroll = 1;
        }
        if step >= 5 {
            m.result = "Synthetic result".into();
        }
        let page = if step == 6 { Page::LockedOut(["Synthetic lockout".into(), String::new()]) } else { Page::Menu(m) };
        TerminalScreen(Some(Session { reference: FormId(41), base: FormId(42), page }))
    }

    #[test]
    fn detect_terminal_navigation_resets_but_ineffective_presses_fire() {
        for step in 1..=6 {
            navigation_resets_detection(terminal_fixture(step - 1), terminal_fixture(step), terminal_fixture(step), |_| {}, if step == 6 { "HackingMenu" } else { "TerminalMenu" });
        }
    }

    #[test]
    fn detect_hacking_selection_and_results_reset_but_timers_do_not() {
        use nvf_play::terminal::report_progress_fixture as fixture;
        for step in 1..=2 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 2 { 3 } else { step }), |_| {}, "HackingMenu");
        }
    }

    #[test]
    fn detect_builtin_screens_ignore_idle_time_until_presses_arrive() {
        for screen in 0..4 {
            let mut app = detector_app();
            match screen {
                0 => {
                    app.insert_resource(terminal_fixture(0));
                }
                1 => {
                    app.insert_resource(nvf_play::levelup::report_progress_fixture(0));
                }
                2 => {
                    app.insert_resource(nvf_play::dialogue::report_progress_fixture(0));
                    app.insert_resource(State::new(nvf_play::PlayState::Dialogue));
                }
                _ => {
                    let mut pip = nvf_play::pipboy::PipBoy::default();
                    pip.phase = nvf_play::pipboy::Phase::Up;
                    pip.page = nvf_play::pipboy::Page::Data;
                    app.insert_resource(pip);
                    app.insert_resource(nvf_play::pipboy::data::report_progress_fixture(0));
                }
            }
            sample(&mut app, 0.0, 0);
            sample(&mut app, 120.0, 0);
            assert!(app.world().resource::<Collector>().summary.events.is_empty());
            sample(&mut app, 121.0, 5);
            assert_eq!(app.world().resource::<Collector>().summary.events.len(), 1);
        }
    }

    #[test]
    fn detect_levelup_navigation_and_allocations_reset_but_hover_does_not() {
        use nvf_play::levelup::report_progress_fixture as fixture;
        for step in 1..=5 {
            navigation_resets_detection(fixture(step - 1), fixture(step), fixture(if step == 5 { 6 } else { step }), |_| {}, "LevelUpMenu");
        }
    }

    #[test]
    fn detect_dialogue_selection_resets_but_boundary_presses_and_timers_do_not() {
        use nvf_play::dialogue::report_progress_fixture as fixture;
        navigation_resets_detection(
            fixture(0),
            fixture(1),
            fixture(2),
            |w| {
                w.insert_resource(State::new(nvf_play::PlayState::Dialogue));
            },
            "DialogMenu",
        );
    }

    #[test]
    fn detect_data_note_scroll_and_list_navigation_reset_but_counters_do_not() {
        use nvf_play::pipboy::{data::report_progress_fixture as fixture, Page, Phase, PipBoy};
        for step in 1..=3 {
            navigation_resets_detection(
                fixture(step - 1),
                fixture(step),
                fixture(if step == 3 { 4 } else { step }),
                |w| {
                    let mut pip = PipBoy::default();
                    pip.phase = Phase::Up;
                    pip.page = Page::Data;
                    pip.tabs[Page::Data.index()] = 3;
                    w.insert_resource(pip);
                },
                "Pip-Boy Data",
            );
        }
    }

    #[derive(Resource, Default)]
    struct Sample {
        screen: u64,
        presses: usize,
    }

    #[test]
    fn engine_menu_pages_reset_the_watch() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).insert_resource(nvf_play::pause::Paused(true)).init_resource::<nvf_play::pause::Page>().init_resource::<Sample>().add_systems(
            Update,
            |menus: Menus, mut sample: ResMut<Sample>| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                assert_eq!(menus.screen(None, &mut hash).as_deref(), Some("StartMenu"));
                sample.screen = hash.finish();
            },
        );
        app.update();
        let first = app.world().resource::<Sample>().screen;
        let mut watch = anomaly::ControlsLocked::default();
        watch.step(0.0, Some(LockProgress { screen: first, groups: [false; 7] }), anomaly::CONTROLS_LOCKED_PRESSES);
        *app.world_mut().resource_mut::<nvf_play::pause::Page>() = nvf_play::pause::Page::Settings { selected: None };
        app.update();
        let second = app.world().resource::<Sample>().screen;
        assert_ne!(first, second);
        assert_eq!(watch.step(59.0, Some(LockProgress { screen: second, groups: [false; 7] }), 0), None);
        assert_eq!(watch.step(60.0, Some(LockProgress { screen: second, groups: [false; 7] }), anomaly::CONTROLS_LOCKED_PRESSES), None);
    }

    #[test]
    fn input_collection_counts_physical_edges_without_synthesized_menu_keys() {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin)).init_resource::<Sample>().add_systems(
            Update,
            (|mut keys: ResMut<ButtonInput<KeyCode>>| keys.press(KeyCode::Enter), |mut presses: Presses, mut sample: ResMut<Sample>| sample.presses = presses.count()).chain(),
        );
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, GamepadButton::South, 1.0)));
        app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window: Entity::PLACEHOLDER });
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyE,
            logical_key: Key::Character("e".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
        assert_eq!(app.world().resource::<Sample>().presses, 3);
        app.update();
        assert_eq!(app.world().resource::<Sample>().presses, 0, "held buttons have no new edges");
    }

    #[test]
    fn keyboard_repeats_and_releases_are_not_presses() {
        let mut event =
            KeyboardInput { key_code: KeyCode::KeyE, logical_key: Key::Character("e".into()), state: ButtonState::Pressed, text: None, repeat: false, window: Entity::PLACEHOLDER };
        assert!(key_press(&&event));
        event.repeat = true;
        assert!(!key_press(&&event));
        event.repeat = false;
        event.state = ButtonState::Released;
        assert!(!key_press(&&event));
    }

    /// #3044: only the lock's quest and the quests whose stage moved since the hold began,
    /// bounded; quests that sat still are left out.
    #[test]
    fn the_lock_s_quest_and_moved_quests_are_listed_bounded() {
        let mut quests = std::collections::BTreeMap::new();
        for i in 0..30 {
            let mut quest = fnv_game::quest::QuestState { running: true, ..Default::default() };
            quest.stages_done.insert(10);
            quests.insert(fnv_sim::FormId(i), quest);
        }
        let at_start: std::collections::HashMap<_, _> = quests.iter().map(|(id, q)| (*id, q.stage())).collect();
        // Nothing moved, no lock quest: an empty list.
        assert!(quest_stages(quests.iter(), None, &at_start).is_empty());
        // Quest 3 moved; quest 7 holds the lock.
        quests.get_mut(&fnv_sim::FormId(3)).unwrap().stages_done.insert(20);
        let stages = quest_stages(quests.iter(), Some(fnv_sim::FormId(7)), &at_start);
        assert_eq!(stages, vec![json!({ "quest": "00000003", "stage": 20, "owns_lock": false }), json!({ "quest": "00000007", "stage": 10, "owns_lock": true })]);
        // Every quest moved: bounded.
        for q in quests.values_mut() {
            q.stages_done.insert(30);
        }
        assert_eq!(quest_stages(quests.iter(), None, &at_start).len(), anomaly::CONTROLS_LOCKED_QUESTS);
    }

    /// #3044: the DisablePlayerControls caller is named (its unit, quest and reference).
    #[test]
    fn the_lock_names_the_script_that_called_disable_player_controls() {
        let caller = fnv_game::game::ControlsCaller { unit: "VCG01 stage 10".into(), quest: Some(fnv_sim::FormId(0x0010_4C1C)), reference: Some(fnv_sim::FormId(0x14)) };
        let (sources, _) = sources([true, false, false, false, false, false, false], Some(&caller), &Default::default());
        assert_eq!(sources[0]["script"], "VCG01 stage 10");
        assert_eq!(sources[0]["quest"], "00104C1C");
        assert_eq!(sources[0]["ref"], "00000014");
    }

    /// #3044: the pause menu is the player's own: a minute in it with key presses and the
    /// controls held fires nothing; the same minute with it closed fires.
    #[test]
    fn the_pause_menu_does_not_count_as_a_lock() {
        let mut app = detector_app();
        app.insert_resource(nvf_play::pause::Paused(true)).init_resource::<nvf_play::pause::Page>();
        sample(&mut app, 0.0, 0);
        sample(&mut app, 59.0, 5);
        sample(&mut app, 130.0, 5);
        assert!(app.world().resource::<Collector>().summary.events.is_empty(), "paused: no controls_locked");
    }

    #[test]
    fn labels_and_group_names_are_bounded() {
        assert_eq!(cut(&"é".repeat(200)).chars().count(), anomaly::CONTROLS_LOCKED_LABEL_CHARS);
        assert_eq!(groups([true, false, false, false, true, false, false]), ["movement", "looking"]);
        assert_eq!(menu_name(fnv_game::game::menus::MESSAGE), "MessageMenu");
        assert_eq!(menu_name(65535), "Menu 65535");
    }

    #[test]
    fn lock_sources_keep_known_provenance_and_cap_plugin_labels() {
        let mut alt = std::collections::BTreeMap::new();
        alt.insert("inactive".into(), [false; 7]);
        for i in 0..20 {
            alt.insert(format!("{i:02}{}", "x".repeat(200)), [true; 7]);
        }
        let (sources, omitted) = sources([true, false, false, false, false, false, false], None, &alt);
        assert_eq!(sources.len(), anomaly::CONTROLS_LOCKED_SOURCES);
        assert_eq!(omitted, 11);
        assert_eq!(sources[0]["by"], "DisablePlayerControls");
        assert_eq!(sources[0]["script"], Value::Null);
        assert_eq!(sources[0]["groups"], json!(["movement"]));
        assert_eq!(sources[1]["by"], "DisablePlayerControlsAlt");
        assert_eq!(sources[1]["plugin"].as_str().unwrap().len(), anomaly::CONTROLS_LOCKED_LABEL_CHARS);
    }

    #[test]
    fn new_dialogue_lines_and_choices_are_progress_but_countdowns_are_not() {
        use fnv_game::dialogue::{Choice, Conversation, Line};
        use fnv_sim::FormId;
        let fingerprint = |c: &Conversation| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            dialogue_screen(1, Some(c), &mut hash);
            hash.finish()
        };
        let mut c = Conversation {
            actor: FormId(42),
            through: None,
            line: Some(Line { topic: FormId(43), info: FormId(44), response: 0, remaining: 10.0 }),
            choices: Vec::new(),
            topic_list: false,
            lines: 1,
            then: None,
            ending: false,
        };
        let first = fingerprint(&c);
        c.line.as_mut().unwrap().remaining = 9.0;
        assert_eq!(first, fingerprint(&c));
        c.line.as_mut().unwrap().response = 1;
        assert_ne!(first, fingerprint(&c));
        c.line = None;
        let choosing = fingerprint(&c);
        c.choices.push(Choice { topic: FormId(45), info: FormId(46), chars: 4, prompt: true, goodbye: false, darkened: false, challenge: None, dumb: false });
        assert_ne!(choosing, fingerprint(&c));
        c.topic_list = true;
        assert_ne!(choosing, fingerprint(&c));
        c.lines += 1;
        assert_ne!(choosing, fingerprint(&c));
    }
}
