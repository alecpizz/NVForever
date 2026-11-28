//! The tile tree beside the drawn menus [design: ui-tile-tree.md §6.2 step 1].
//!
//! Measuring the writes (issue #1457): the span profile (`nvforever::profile`) sees
//! `tiles::write` as a system and these parts of it, as `tiles::write::<part>`. Top level:
//! `sync` (`screen()` and the menus' lifetime), `menus` (the T15 menus and the Mods entry),
//! `shown`, `texts`, `animation`, and then either `widths` (the HUD's writes kept back, the
//! default game) or `hud_write`, `text_widths`, `corners`, `notices`, `breath`. Inside
//! those: `menu_drop`, `menu_build`, `menu_update` in `sync` (`fnv_game::ui_loading`),
//! `subtitles` in `shown`, `prompt` in `texts`, `font_load` wherever a face is first made.
//! Entries say what was tried, not what came of it: `menu_build` is entered for a menu with
//! no file and for a load that fails, and `font_load` for a face that ends as the stand-in.
//! Parts nest, so don't add a part to its parent. `tiles::write` less the top-level parts
//! is what no part names plus the spans' own cost and, with the counts on, their line.
//!
//! The spans are in every build, with no profile collector too [design]. `LogPlugin` keeps
//! its default `INFO` (unless `RUST_LOG` says otherwise), so they are enabled: 8 a frame on
//! the default game's path, up to 12 when the HUD is written, and `menu_drop`, `menu_build`,
//! `menu_update` and `font_load` only on the frames doing that work. Each takes a slot in
//! `tracing_subscriber`'s `Registry`, a push and a pop on the thread's span stack, a look
//! at `EnvFilter`'s span map on entering and leaving, and the fmt layers' `on_new_span`,
//! which keep an empty `FormattedFields` in the span's extensions (Bevy's fmt layer and
//! `engine/nvforever/src/crash.rs` `file_layer`). They print nothing themselves, but a log
//! line made inside one now names it. One span's create, enter, exit and close took about
//! 440 ns in a scratch benchmark under the same kind of subscriber stack, which estimates
//! 3.5 to 5.3 µs at 8 to 12 a frame; frame times of whole runs could not resolve a cost at
//! that size [observed 2026-11-04, #1511].
//!
//! `NVF_PLAY_TEST_TILE_COUNTS=1` (a test aid of its own, in any build; time without it, its
//! line lies outside the parts): a line a frame, `tile counts: writes=<n> changed=<n>
//! speech=<n> faces=<n> measure_candidates=<n> deferred=<bool>`: the trait writes this
//! system made and how many changed a value, `game.speech`'s lines (those `subtitles` goes
//! over; a line not heard stops at its first test), the faces made so far (the game's fonts
//! and stand-ins alike), the HUD text tiles a width could be measured for on a frame that
//! writes the HUD (no count of measures: no game font means none, a tile without a string
//! is passed over; 0 when deferred, the measuring waits), and whether the HUD's writes were
//! kept back. Off, one cached flag a frame is read here and the tree's two counters advance
//! anyway ([`fnv_ui::Ui::trait_write_counts`]).

pub mod container_menu;
pub mod barter_menu;
pub mod recipe_menu;
pub mod repair_cards;
pub mod repair_menu;
pub mod repair_services_menu;
pub mod runtime_images;
pub mod local_map_service;
pub mod projected_ui;
pub mod modern_minimap;
mod item_menu;
pub mod dialog_menu;
pub mod draw;
mod global_image_metrics;
pub mod input;
pub mod inventory_menu;
pub mod item_card;
pub mod item_mod_menu;
pub mod layout;
#[cfg(test)]
mod quick_loot_hud_alpha_tests;
pub mod list;
pub mod map_menu;
pub mod menus;
pub mod model;
pub mod mods;
pub mod notices;
pub mod stats_menu;
pub(crate) mod selection_menu;
pub mod tag_menu;
pub mod texmod;
pub mod trait_menu;
pub mod tutorial_menu;
pub mod vanilla;
pub mod vats_menu;
pub mod wait_menu;

use crate::{script::ScriptState, PlaySet, PlayState};
use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};
use fnv_game::ui_loading::{self, UiFiles};
use fnv_ui::Value;
use nvf_assets::GameData;

#[derive(SystemParam)]
pub(crate) struct MenuFiles<'w, 's> {
    data: Option<Res<'w, GameData>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    main_menu: Option<Res<'w, crate::main_menu::MainMenu>>,
    menus: Option<Res<'w, vanilla::TileMenus>>,
    page: Option<Res<'w, crate::pause::Page>>,
    pads: Query<'w, 's, (), With<Gamepad>>,
}

/// `globals()`'s `_Has360Controller`, a trait the code keeps up to date: 1 while a
/// controller is connected, 0 otherwise, written only where a file gives `globals()` the
/// trait [observed: Vanilla UI Plus's globals.xml sets it to 1 with the note that unlike
/// `&xbox;` it "gets updated in-game", and reads it to support a controller plugged in or
/// out; hypothesis: connected, not last used]. Left at the file's 1, MCM (through MCM
/// Extender's `_AltInput`) took every pointer for a controller and ignored its clicks
/// (issue #2298, docs/spec-notes/mcm.md §3).
pub const HAS_CONTROLLER: &str = "_Has360Controller";

/// Writes [`HAS_CONTROLLER`] on `globals()` when the trait is there.
pub fn write_has_controller(ui: &mut fnv_ui::Ui, connected: bool) {
    let globals = ui.globals();
    let v = f64::from(u8::from(connected));
    if ui.tile(globals).get(HAS_CONTROLLER).is_some_and(|now| now.num() != v) {
        ui.set_existing(globals, HAS_CONTROLLER, Value::Num(v));
    }
}

impl MenuFiles<'_, '_> {
    /// Install before startup/load scripts, and retry if GameData arrived later.
    /// Game::restore creates fresh UiExtra, so every new game gets its own cache [design].
    pub(crate) fn install_measure(&self, game: &mut fnv_game::Game<'_>) {
        if game.ui_extra.measure.is_some() {
            return;
        }
        let Some(data) = self.data.as_ref() else { return };
        let vfs = data.vfs.clone();
        let extra = crate::interface::load_xfonts(&vfs);
        let load_extra = extra.clone();
        game.ui_extra.measure = Some(draw::string_measure_with_fonts(extra, move |number| {
            let path = crate::interface::slot_font_path(number, &load_extra)?;
            let bytes = vfs.read(&path)?.ok()?;
            fnv_font::Font::parse(&bytes).ok()
        }));
    }

    /// Seed SUP/JIP window and cursor state from the primary window before the first
    /// startup/load scripts run (`PreUpdate` ran before `ScriptState::rt` existed) [design].
    pub(crate) fn sync_window_and_cursor(&self, game: &mut fnv_game::Game<'_>) {
        if let Ok(window) = self.windows.single() {
            game.sup.window = (f64::from(window.physical_width()), f64::from(window.physical_height()));
            if let Some(p) = window.physical_cursor_position() {
                game.sup.mouse.pos = (f64::from(p.x), f64::from(p.y));
            }
            game.other.jip9.window = Some((f64::from(window.width()), f64::from(window.height())));
            game.other.jip9.cursor_px = window.cursor_position().map(|p| (f64::from(p.x), f64::from(p.y)));
        }
    }

    pub(crate) fn load(&self, game: &mut fnv_game::Game<'_>, world: &fnv_sim::World<'_>) {
        self.sync_window_and_cursor(game);
        let Some(data) = self.data.as_ref() else { return };
        let files = UiFiles::new(data.vfs.clone(), world);
        let paths = files.menu_paths();
        let report = ui_loading::load(game, Box::new(files), paths);
        self.write(game);
        game.ui_tree.update();
        for (file, messages) in &report.warnings {
            warn!("ui: {file}: {}", messages.join("; "));
        }
        info!("ui: {} menus loaded ({} tiles), {} warnings", report.menus, report.tiles, report.warning_count());
        // T10 (menu-xml.md §2.4): names with no GMST record read NVForever's own labels.
        info!("ui: game strings: {} read, {} without text", report.game_strings, report.game_strings_unresolved);
    }

    /// The pointer's place in the window, logical pixels; none while it is outside.
    pub(crate) fn pointer(&self) -> Option<(f32, f32)> {
        self.windows.single().ok().and_then(|w| w.cursor_position()).map(|p| (p.x, p.y))
    }

    /// The window's size, logical pixels.
    pub(crate) fn window_size(&self) -> Option<(f32, f32)> {
        self.windows.single().ok().map(|w| (w.width(), w.height()))
    }

    pub(crate) fn write(&self, game: &mut fnv_game::Game<'_>) {
        self.install_measure(game);
        if game.ui_files.is_none() {
            return;
        }
        if let Ok(window) = self.windows.single() {
            // T15a: interface units only while a T15 menu is drawn from its file.
            // Also while the classic VATS screen is the game's file (`vats_menu`), which
            // places its pieces by `screen()` as those menus do.
            // And while the start menu's Mods page is up (`mods`): MCM's window centres
            // itself by `screen()` [observed: menu-xml.md §10.1].
            let mods_page = self.menus.as_ref().is_some_and(|m| m.start) && self.paused.as_deref().zip(self.page.as_deref()).is_some_and(|(p, pg)| crate::pause::mods_open(p, pg));
            let units = self.menus.as_ref().is_some_and(|m| interface_screen(**m)) || game.ui_vats || mods_page;
            if game.ui_vats {
                write_screen_scaled(&mut game.ui_tree, window.width(), window.height(), crate::interface::menu_scale());
            } else {
                write_screen(&mut game.ui_tree, window.width(), window.height(), units);
            }
        }
        let report = ui_loading::sync(game, self.paused.as_ref().is_some_and(|p| p.0), self.main_menu.as_ref().is_some_and(|m| m.open));
        write_has_controller(&mut game.ui_tree, !self.pads.is_empty());
        for line in report.changes {
            info!("ui: {line}");
        }
        for line in report.warnings {
            warn!("ui: {line}");
        }
    }
}

/// Whether `screen()` is written in interface units ([`write_screen`]): only while a T15
/// menu (`MessageMenu`, `QuantityMenu`, `SleepWaitMenu`, `TutorialMenu`, `ContainerMenu`, `TraitMenu`, `CharGenMenu`, `DialogMenu`) is switched on,
/// whose files centre their boxes by `screen()`, or `StatsMenu`, whose lines take their thickness from the
/// converter and whose hotrects' `alpha` 0 this same gate lets pass (pipboy.md §5.1.1). The
/// default game and the tile HUD alone keep the window's pixels [design]. The gate is one for
/// the whole tree, not a menu's own: `StatsMenu` beside the tile HUD changes the shared screen
/// traits and the HUD's hotrect-alpha handling, as any of these does, and beside another of
/// them changes nothing more. The Pip-Boy's canvas is never written here. The original reports
/// interface units [observed: hud_menus capture 2026-10-31, three window sizes]; applying them
/// unconditionally is a follow-up. The start menu's switch is not one of them: it is on by
/// default, and its Mods page asks for interface units only while it is up
/// ([`MenuFiles::write`]).
pub fn interface_screen(menus: vanilla::TileMenus) -> bool {
    if menus.repair_services || menus.repair_menu || menus.item_mod {
        return true;
    }
    menus.message || menus.quantity || menus.sleep_wait || menus.tutorial || menus.container || menus.barter || menus.recipe || menus.traits || menus.tags || menus.stats || menus.dialog || menus.inventory || menus.map
}

/// Write `screen()`'s traits for a window `window_width` x `window_height` (logical
/// pixels): with `interface_units` ([`interface_screen`]) the view in interface units
/// ([`screen_size`]) and `resolutionconverter` ([`resolution_converter`]); otherwise the
/// window's size, as fnvtool's `--screen`, and no converter, as before T15a [design:
/// menu-xml.md §4.2, `screen()` row]. `cropx` and `cropy` stay unset until their units are
/// known, but while the dialogue menu's file reads them (`dialog_menu`) [design]. The HUD's places are worked out from the shape alone
/// (`fnv_game::ui_hud::write`), so they are the same either way.
pub fn write_screen(ui: &mut fnv_ui::Ui, window_width: f32, window_height: f32, interface_units: bool) {
    let id = ui.screen();
    if interface_units {
        let (width, height) = screen_size(window_width, window_height);
        ui.set_trait(id, "width", Value::Num(width));
        ui.set_trait(id, "height", Value::Num(height));
        ui.set_trait(id, "resolutionconverter", Value::Num(resolution_converter(window_height)));
    } else {
        ui.set_trait(id, "width", Value::Num(f64::from(window_width)));
        ui.set_trait(id, "height", Value::Num(f64::from(window_height)));
    }
}

/// [`write_screen`] in interface units for a view drawn at `scale` (the UI scale, at most
/// 100 %: `crate::interface::menu_scale`, #1631) in the window's own frame, as the HUD is: the view is
/// 960 / scale units high, so a file that places its pieces by `screen()` keeps them at
/// the window's edges at any scale. Written while the VATS target screen is up
/// (`vats_menu`), which is drawn so.
pub fn write_screen_scaled(ui: &mut fnv_ui::Ui, window_width: f32, window_height: f32, scale: f32) {
    let id = ui.screen();
    let s = if scale.is_finite() && scale > 0.0 { f64::from(scale) } else { 1.0 };
    let (width, height) = screen_size(window_width, window_height);
    ui.set_trait(id, "width", Value::Num(width / s));
    ui.set_trait(id, "height", Value::Num(height / s));
    ui.set_trait(id, "resolutionconverter", Value::Num(resolution_converter(window_height) / s));
}

/// `screen()`'s width and height for a window in interface units: 960 high and
/// 960 × window width/height wide [observed: hud_menus capture 2026-10-31, three window sizes].
/// The windows were 1280 x 720, 1920 x 1080 and 1280 x 1024 (also hud.md §14's
/// right-hand pieces at W - 386). Written only while a T15 menu is switched on ([`interface_screen`]);
/// applying these observed units unconditionally is a follow-up [design].
pub fn screen_size(window_width: f32, window_height: f32) -> (f64, f64) {
    let (w, h) = (f64::from(window_width), f64::from(window_height));
    if w > 0.0 && h > 0.0 {
        (960.0 * w / h, 960.0)
    } else {
        (0.0, 0.0)
    }
}

/// `screen()`'s `resolutionconverter` for a window `window_height` pixels high:
/// 960 / height [observed: hud_menus capture 2026-10-31, three window sizes]. The windows
/// were 1280 x 720, 1920 x 1080 and 1280 x 1024. Thus `globals()`'s PC `_line_thickness` (2 × the
/// converter) is 2 pixels thick [documented: globals.xml]. Written only with interface
/// units ([`write_screen`]); the unconditional switch is a follow-up [design].
pub fn resolution_converter(window_height: f32) -> f64 {
    if window_height > 0.0 {
        960.0 / f64::from(window_height)
    } else {
        0.0
    }
}

/// Read the same inputs as the current HUD, before scripts [design]. Timed pieces
/// use the current target/queue/meter state; their clocks remain with the drawing.
#[derive(SystemParam)]
struct HudTiles<'w> {
    state: Res<'w, State<PlayState>>,
    sim: Res<'w, crate::sim::SimState>,
    control: Option<Res<'w, crate::control::PlayerControl>>,
    locks: Option<Res<'w, crate::control::ControlLocks>>,
    driven: Option<Res<'w, crate::player::ViewDriven>>,
    wheel: Option<Res<'w, crate::weapon_wheel::WeaponWheel>>,
    creator: Option<Res<'w, crate::creator::CreatorMenu>>,
    zoom: Option<Res<'w, crate::aim_zoom::AimZoom>>,
    sights: Option<Res<'w, crate::aim_zoom::TrueSights>>,
    scope: Option<Res<'w, crate::aim_zoom::Scope>>,
    combat: Option<Res<'w, crate::combat::Combat>>,
    target: Option<Res<'w, crate::activate::Target>>,
    bindings: Option<Res<'w, crate::bindings::Bindings>>,
    enemy: Option<Res<'w, crate::hud_target::HudTarget>>,
    vats: Option<Res<'w, crate::vats_mode::VatsMode>>,
    aim_hostile: Option<Res<'w, crate::hud_target::AimHostile>>,
    tweaks: Option<Res<'w, crate::tweaks::TweakSettings>>,
    limbs: Option<Res<'w, crate::hud_limbs::Indicator>>,
    player_limbs: Option<Res<'w, crate::combat::PlayerLimbs>>,
    breath: Option<Res<'w, crate::drowning::PlayerBreath>>,
    xp: Option<Res<'w, crate::xp_meter::Meter>>,
    corner: Option<Res<'w, crate::corner_messages::CornerMessages>>,
    quests: Option<Res<'w, crate::quest_messages::QuestMessages>>,
    hud: Option<Res<'w, crate::script_hud::ScriptHud>>,
    player: Option<Res<'w, crate::player::Player>>,
    panel: Option<Res<'w, crate::message_menu::MessagePanel>>,
    loading: Option<Res<'w, crate::loading::Loading>>,
    bars: Option<Res<'w, crate::hud::HudView>>,
    compass: Option<Res<'w, crate::compass::CompassView>>,
}

impl HudTiles<'_> {
    fn shown(&self, game: &fnv_game::Game<'_>, files: &MenuFiles<'_, '_>) -> fnv_game::ui_hud::Shown {
        let playing = *self.state.get() == PlayState::Playing;
        let root = playing
            && !game.in_menu()
            && !files.paused.as_ref().is_some_and(|p| p.0)
            && !files.main_menu.as_ref().is_some_and(|m| m.open)
            && !self.loading.as_ref().is_some_and(|l| l.active());
        // A playing kill camera hides all of it, as an idle holding the camera does
        // (`crate::kill_cam`) [owner's playtest request #1034].
        let driven = self.driven.as_ref().is_some_and(|d| d.0) || self.bars.as_ref().is_some_and(|b| b.kill_cam);
        let wheel = self.wheel.as_ref().is_some_and(|w| w.is_open());
        let creating = self.creator.as_ref().is_some_and(|c| c.is_open());
        let locks = self.locks.as_deref().copied().unwrap_or_default();
        // hud::gather and hud::{draw,texts}: the opening's movement lock, portrait,
        // held camera and weapon wheel hide the corner HUD and its meters [design].
        let corners = playing && !locks.movement && !self.control.as_ref().is_some_and(|c| c.portrait.is_some()) && !driven && !wheel;
        let weapon = self.combat.as_ref().map(|c| c.hud_weapon(&self.sim)).unwrap_or_default();
        // [design, #2659] Classic targets show even at full health, through playback.
        let classic = self.vats.as_ref().is_some_and(|v| v.style() == crate::vats_classic::VatsStyle::Classic && v.active());
        let enemy = self.enemy.as_ref().and_then(|e| e.0.as_ref()).filter(|e| classic || e.health < 1.0);
        // reticle::draw: the prompt also goes under true sights/the scope, and when
        // the enemy bar names this target; only the crosshair obeys movement [design].
        let reticle = playing && !creating && !driven && !wheel && !self.sights.as_ref().is_some_and(|s| s.0) && !self.scope.as_ref().is_some_and(|s| s.on);
        let info = reticle && !locks.rollover && self.target.as_ref().and_then(|t| t.0.as_ref()).is_some_and(|t| t.prompted() && !enemy.is_some_and(|e| crate::reticle::under_bar(t, e.form_id)));
        // corner_messages::draw and quest_messages::draw [design].
        let notices = !driven && *self.state.get() != PlayState::Dialogue && !self.panel.as_ref().is_some_and(|p| p.view.is_some());
        let quest = self.quests.as_ref().and_then(|q| q.showing());
        // Map discovery to Region_Location [hypothesis: hud.md §14; one queue here]. The
        // tile HUD writes it under both: Region_Location's copies hold its traits (not
        // drawn), QuestReminder's are drawn, where the owner's screenshot shows it
        // (`notices`, hud.md §14.11).
        let location = matches!(quest, Some(crate::script_events::QuestNotice::Discovered(_)));
        // hud_needs::watch/draw: H20/FOD/SLP in Hardcore, RAD and LMB in every mode
        // [design: hud.md §13; observed words: hud.md §14.2].
        // The rect shows if any word has a look other than Hidden [design].
        use fnv_game::aid::{DEHYDRATION, HUNGER, RAD_LEVEL, SLEEP_DEPRIVATION};
        let need = |code| game.player.need_stages.get(&code).is_some_and(|held| game.data.need_stages.get(&code).is_some_and(|stages| stages.iter().any(|s| s.stage == *held)));
        // needs::hardcore with the live world's config, as hud_needs::watch [design].
        let hardcore = game.player.hardcore.unwrap_or(self.sim.world.config().hardcore);
        // Only the six limbs count, as hud_needs::watch (the brain is excluded) [design].
        let needs = need(RAD_LEVEL)
            || (hardcore && [DEHYDRATION, HUNGER, SLEEP_DEPRIVATION].into_iter().any(need))
            || self.player_limbs.as_ref().is_some_and(|l| (25..=30).any(|code| l.crippled(code)));
        fnv_game::ui_hud::Shown {
            root,
            aiming: playing && self.zoom.as_ref().is_some_and(|z| z.aimed),
            corners,
            sneak: corners && self.control.as_ref().is_some_and(|c| c.sneak) && !self.tweaks.as_ref().is_some_and(|t| t.no_sneak_label),
            reticle: reticle && !locks.movement && !self.tweaks.as_ref().is_some_and(|t| t.crosshair_hidden),
            enemy: (corners || (classic && self.bars.as_ref().is_some_and(|b| b.enemy_shown))) && enemy.is_some(),
            quests: notices && quest.is_some() && !location,
            messages: notices && self.corner.as_ref().is_some_and(|c| c.showing().is_some()),
            subtitles: {
                let _span = info_span!("tiles::write::subtitles").entered();
                !creating && self.hud.as_ref().is_some_and(|h| crate::script_hud::hud_subtitles_shown(h, game, &self.sim, self.player.as_deref()))
            },
            info,
            location: notices && location,
            xp: corners && self.xp.as_ref().is_some_and(|m| m.show.is_some()), // xp_meter::draw [design].
            radiation: corners && game.radiation().rate > 0.0,                 // radiation::draw [design].
            breath: corners && self.breath.as_ref().is_some_and(|b| crate::hud_breath::shown(b)),
            limbs: corners && self.limbs.as_ref().is_some_and(|l| l.shown()), // hud_limbs::draw [design].
            needs: corners && needs,
            ammo_label: corners && weapon.ammo.is_some() && weapon.abbrev.as_ref().is_some_and(|s| !s.is_empty()),
            condition: corners && weapon.condition.is_some(),
        }
    }

    /// What the tile-drawn corners take beyond T4's writes (`vanilla::write_corners`): the
    /// hand-built HUD's label places as the copies' tops and edges, the right piece's moved
    /// from its `MARGIN_X` in to the tile piece's place on the view's edge [observed: hud.md
    /// §14, `ActionPoints` at W - 386; design: `hud::label_places`], and how far the
    /// condition row moves for the ammo abbreviation (its width in font 7).
    fn corner(&self, shown: fnv_game::ui_hud::Shown, fonts: &mut draw::FontKit) -> vanilla::Corner {
        let (labels, line) = fonts.hud_labels();
        let shift = crate::hud::MARGIN_X;
        let top = |(x, base): (f32, f32), shift: f32| (f64::from(x - shift), f64::from(base - line));
        let weapon = self.combat.as_ref().map(|c| c.hud_weapon(&self.sim)).unwrap_or_default();
        // The row's place follows the weapon even while the corners are hidden (a menu,
        // the console), as the pieces' places hold [design: hud.md §14.1].
        let abbrev = weapon.abbrev.clone().filter(|_| weapon.ammo.is_some()).unwrap_or_default();
        let condition_shift = vanilla::condition_shift(&abbrev, f64::from(fonts.hud_width(&abbrev)));
        let ammo_label = if shown.ammo_label { abbrev } else { String::new() };
        vanilla::Corner {
            hp: top(labels[0], 0.0),
            ap: top(labels[1], shift),
            cnd: top(labels[2], shift),
            ammo: top(labels[3], shift),
            right_bracket: vanilla::RIGHT_BRACKET_X,
            condition_shift,
            ammo_label,
            condition: shown.condition,
        }
    }

    /// What the tile-drawn messages, quest notices and subtitles show (`notices::write`):
    /// the hand-built queues' lines on screen with their strengths, and the subtitles the
    /// hand-built HUD draws, under the same showing rules as [`HudTiles::shown`]'s
    /// (`messages`, `quests`/`location`, `subtitles`) [design: their rules stay the
    /// hand-built HUD's].
    fn notices(&self, game: &fnv_game::Game<'_>, shown: fnv_game::ui_hud::Shown) -> notices::Notices {
        use crate::script_events::QuestNotice;
        let message = self.corner.as_ref().filter(|_| shown.messages).and_then(|c| {
            let note = c.showing()?;
            Some(notices::Message {
                text: note.text.clone(),
                icon: note.icon.clone().unwrap_or_else(|| crate::corner_messages::DEFAULT_ICON.into()),
                strength: f64::from(c.strength()),
            })
        });
        let quest = self.quests.as_ref().filter(|_| shown.quests || shown.location).and_then(|q| {
            let n = q.showing()?;
            let strength = f64::from(q.strength(0.0));
            Some(match n {
                QuestNotice::Objectives(list) => notices::Quest::Objectives { list: list.iter().map(|o| (crate::quest_messages::objective_text(o), o.completed)).collect(), strength },
                QuestNotice::Added(name) | QuestNotice::Completed(name) | QuestNotice::Failed(name) | QuestNotice::Discovered(name) => {
                    let discovered = matches!(n, QuestNotice::Discovered(_));
                    notices::Quest::Banner { label: crate::quest_messages::label(n).unwrap_or_default().into(), name: name.clone(), strength, discovered }
                }
            })
        });
        let subtitles = match &self.hud {
            Some(h) if shown.subtitles => crate::script_hud::hud_subtitle_texts(h, game, &self.sim, self.player.as_deref()),
            _ => Vec::new(),
        };
        notices::Notices { message, quest, subtitles }
    }

    fn texts(&self, game: &mut fnv_game::Game<'_>, shown: fnv_game::ui_hud::Shown) -> fnv_game::ui_hud::Texts {
        // Read the drawing's current values before scripts, as for Shown [design:
        // ui-tile-tree.md §4.1]; the strings' file tiles [observed: hud.md §14.2].
        let prompt = {
            let _span = info_span!("tiles::write::prompt").entered();
            self.target.as_ref().and_then(|t| t.0.as_ref()).filter(|_| shown.info && !crate::quick_loot::holds_input()).map(|t| crate::reticle::Prompt::bound(t, &self.sim, self.bindings.as_deref()))
        };
        // Reuse the latest drawing snapshot before scripts [design: ui-tile-tree.md §4.1].
        let weapon = self.combat.as_ref().map(|c| c.hud_weapon(&self.sim)).unwrap_or_default();
        // Hostility as `hud_target::aim` worked it out this frame, so both HUDs agree.
        let enemy = self.enemy.as_ref().and_then(|e| e.0.as_ref()).map(|e| fnv_game::ui_hud::Enemy { name: e.name.clone(), health: f64::from(e.health), hostile: e.hostile });
        use fnv_game::aid::{DEHYDRATION, HUNGER, SLEEP_DEPRIVATION, RAD_LEVEL};
        let need_stages = [DEHYDRATION, HUNGER, SLEEP_DEPRIVATION, RAD_LEVEL].map(|code| {
            game.player.need_stages.get(&code).and_then(|held| game.data.need_stages.get(&code)?.iter().position(|s| s.stage == *held)).map_or(0, |i| i + 1)
        });
        let frame = fnv_game::ui_hud::Frame {
            aim_hostile: self.aim_hostile.as_ref().is_some_and(|h| h.0),
            enemy,
            hardcore: game.player.hardcore.unwrap_or(self.sim.world.config().hardcore),
            need_stages,
            crippled: std::array::from_fn(|i| self.player_limbs.as_ref().is_some_and(|l| l.crippled(25 + i as u16))),
            animation: Default::default(),
            ui_scale: f64::from(crate::interface::ui_scale()),
            health: self.bars.as_ref().map_or(1.0, |v| f64::from(v.health)),
            action_points: self.bars.as_ref().map_or(1.0, |v| f64::from(v.action_points)),
            condition: weapon.condition.map(f64::from),
            ammo: crate::hud::ammo_text(self.vats.as_ref().map_or(weapon.ammo, |v| v.displayed_ammo(weapon.ammo))),
            radiation: fnv_game::functions::actor_value(game, fnv_sim::FormId::PLAYER_REF, fnv_game::aid::RAD_LEVEL).unwrap_or(0.0),
            prompt: prompt.as_ref().map(|p| fnv_game::ui_hud::Prompt {
                name: p.name.clone(),
                weight: p.figures.as_ref().map_or_else(|| "--".into(), |f| f[0].clone()),
                value: p.figures.as_ref().map_or_else(|| "--".into(), |f| f[1].clone()),
                empty: p.note.is_some(),
                item: p.figures.is_some(),
                lock_text: self.target.as_ref().and_then(|t| t.0.as_ref()).map_or_else(|| "Locked".into(), |t| {
                    match t.action {
                        crate::activate::outcome::Action::Locked { level } => format!("[Locked - {}]", crate::activate::outcome::lock_text(level)),
                        crate::activate::outcome::Action::UsePassword => "[Locked - Use Password]".into(),
                        _ => "Locked".into(),
                    }
                }),
                locked: self
                    .target
                    .as_ref()
                    .and_then(|t| t.0.as_ref())
                    .is_some_and(|t| matches!(t.action, crate::activate::outcome::Action::Locked { .. } | crate::activate::outcome::Action::UsePassword)),
                // Only the two captured heights; further lines/wrapping await text
                // layout [hypothesis: hud.md §14.3; design: widths deferred].
                height: if p.name.lines().count() > 1 { 80.0 } else { 42.0 },
            }),
            marks: self.compass.as_ref().map_or_else(Vec::new, |view| {
                view.0
                    .iter()
                    // The tile HUD has no door icon: the Show Doors on Compass tweak's marks
                    // are the hand-built HUD's.
                    .filter(|(kind, _)| *kind != crate::compass::Mark::Door)
                    .map(|(kind, offset)| {
                        use crate::compass::Mark;
                        use fnv_game::ui_hud::MarkKind;
                        fnv_game::ui_hud::CompassMark {
                            kind: match kind {
                                Mark::Actor | Mark::Companion => MarkKind::Actor,
                                Mark::Hostile => MarkKind::Hostile,
                                Mark::Place => MarkKind::Place,
                                Mark::Found => MarkKind::Found,
                                Mark::Quest => MarkKind::Quest,
                                Mark::Player => MarkKind::Player,
                                Mark::Door => MarkKind::Place,
                            },
                            heading: 139.0 + f64::from(*offset), // [observed: hud.md §14.5].
                            distance: 1.0,                       // [hypothesis: hud.md §14.6; drawing stores no distances].
                        }
                    })
                    .collect()
            }),
        };
        fnv_game::ui_hud::Texts {
            radiation_word: crate::radiation::WORD.into(),
            radiation_value: crate::radiation::value_text(game.radiation().resisted),
            xp_label: crate::xp_meter::XP_WORD.into(),
            xp_level_up: crate::xp_meter::LEVEL_UP_WORD.into(),
            needs: crate::hud_needs::texts(&self.sim),
            sneak: shown.sneak.then(|| crate::hud::sneak_now(game).word().to_string()),
            prompt_key: prompt.as_ref().map(|p| p.key_text().to_string()),
            prompt_verb: prompt.map(|p| if frame.prompt.as_ref().is_some_and(|p| p.locked) { "Open".into() } else { p.action }),
            frame,
        }
    }
}

/// What the message box and the prompt drawn from tiles were last written from (T15a,
/// `menus`): the menu's tile (a new one when it is built again) and what it showed, so a
/// frame with nothing changed writes nothing [design: the maintainer's review of #902].
#[derive(Default)]
struct MenusWritten {
    message: Option<(menus::BoxShown, Option<usize>)>,
    quantity: Option<(fnv_ui::TileId, menus::Amount)>,
    quantity_text_scale:Option<f32>,
    /// T15b: the Wait / Sleep menu.
    sleep_wait: Option<wait_menu::Shown>,
    /// The text sizes written, the message box's, the prompt's, the Wait menu's and the
    /// start menu's.
    sizes: [menus::SizesWritten; 4],
    /// [design] Each fallback reason is warned once per session, including after reopen.
    fallbacks: std::collections::HashSet<String>,
}

impl MenusWritten {
    fn warn_fallback(&mut self, menu: &str, status: menus::Tiled) {
        if let menus::Tiled::No(reason) = status {
            let line = format!("{menu} drawn by hand: {reason}");
            if self.fallbacks.insert(line.clone()) {
                warn!("{line}");
            }
        }
    }
}

/// T15a: what the message box and the prompt show, from their hand-built models.
#[derive(SystemParam)]
struct MenuTiles<'w, 's> {
    panel: Option<Res<'w, crate::message_menu::MessagePanel>>,
    active: Option<Res<'w, crate::menu::ActiveMenu>>,
    readability:Option<Res<'w,crate::menu_readability::MenuReadability>>,
    /// T15b: the Wait / Sleep menu and its words.
    wait: Option<Res<'w, crate::wait::WaitMenu>>,
    words: Option<Res<'w, crate::wait::model::Words>>,
    /// The ITEMS page's decision, for its prompt (`inventory_menu`).
    inventory: Option<Res<'w, inventory_menu::InventoryTiles>>,
    written: Local<'s, MenusWritten>,
}

impl MenuTiles<'_, '_> {
    /// The "How many?" prompt of the container, barter and inventory menus, when open.
    fn quantity(&self) -> Option<crate::menu::model::Quantity> {
        self.active.as_ref().and_then(|a| a.0.as_ref()).and_then(|m| m.quantity)
    }

    /// Write the switched-on menus' traits (`menus::write_message`, `menus::write_quantity`)
    /// when what they show changed, and their text tiles' sizes every frame they are drawn
    /// (each measured again only when its string changed), before scripts [design:
    /// ui-tile-tree.md §4.1]. Off, or without the menu files, nothing.
    fn write(&mut self, game: &mut fnv_game::Game<'_>, switch: vanilla::TileMenus, fonts: &mut draw::FontKit, calendar: Option<fnv_sim::clock::Calendar>) {
        if !(switch.message || switch.quantity || switch.sleep_wait || switch.container || switch.barter || switch.inventory || switch.start) || game.ui_files.is_none() {
            return;
        }
        // T11: the start menu's text sizes while a mod's tiles are in it, as the code
        // measures them: MCM's mod-list rows take their height from a measured text
        // (`MCM_ModList_Size`, ModListRect.xml) and GetMCMListWidth reads the rows' text
        // widths [observed: menu-xml.md §10.2]. The start menu is built only while the
        // pause menu is open.
        if switch.start {
            if let Some(menu) = game.ui_tree.menu(mods::START_MENU).filter(|&m| game.ui_tree.has_mod_added_tiles(m)) {
                let ui = &mut game.ui_tree;
                let texts = menus::texts(ui, menu);
                let size = fonts.menu_measure(&menus::fonts_of(ui, &texts));
                menus::write_text_sizes_of(ui, &texts, &mut self.written.sizes[3], &size);
            }
        }
        self.write_wait(game, switch, fonts, calendar);
        // [design] Reconsider an overflowing box when its view changes: using the final
        // tiled decision here would prevent a later short box from being written.
        let message_open = switch.message && game.menu == Some(fnv_game::game::menus::MESSAGE);
        let message = (message_open && menus::message_built(&game.ui_tree).yes()).then(|| game.ui_tree.menu(menus::MESSAGE_MENU)).flatten();
        // [design] Over either item menu drawn from tiles the prompt is too. This frame's
        // decision, the one the item-menu writers take after this writer: their menu state
        // here would still be last frame's.
        let shown = self.active.as_ref().and_then(|a| a.0.as_ref());
        let prompt_open = shown.is_some_and(|m| m.quantity.is_some());
        let text_scale=if shown.is_some_and(|m|matches!(m.mode,crate::menu::model::Mode::Barter(_))) {
            crate::menu::readability::size(self.readability.as_deref(),crate::menu_readability::MenuKind::Barter)
        } else {1.0};
        let container_tiled = prompt_open && (container_menu::tiled(switch.container, game, container_menu::is_container(shown))
            || barter_menu::tiled(switch.barter, game, barter_menu::is_barter(shown))
            || self.inventory.as_ref().is_some_and(|i| i.ready.is_some()));
        let quantity = self.quantity().filter(|_| menus::quantity_tiled(switch.quantity, game, true, container_tiled));
        let prompt = game.ui_tree.menu(menus::QUANTITY_MENU).zip(quantity);
        // Which showing the box on screen is (counted up for every box shown, one alike
        // included): its rows are made again for another (`menus::another_box`).
        let showing = game.front_box().map(|b| b.serial);
        let written = &mut *self.written;
        let ui = &mut game.ui_tree;
        match (message, self.panel.as_ref().and_then(|p| p.view.as_ref().map(|v| (v, p.framed())))) {
            (Some(menu), Some((view, selected))) => {
                let now = menus::BoxShown { menu, showing, view: view.clone() };
                let was = written.message.as_ref();
                let another = menus::another_box(was.map(|(b, _)| b), &now);
                if another || was.is_some_and(|(_, s)| *s != selected) {
                    {
                        let size = fonts.menu_measure(&menus::fonts(ui, menu));
                        menus::write_message(ui, view, selected, another, &size);
                    }
                    // The fonts again: the rows are new and the tree was worked out, and a
                    // measure is kept, so a font not loaded yet would stay at 0 x 0.
                    let size = fonts.menu_measure(&menus::fonts(ui, menu));
                    menus::write_text_sizes(ui, menu, &mut written.sizes[0], &size);
                    if another && menus::message_status(ui).yes() {
                        info!("{}", menus::describe_message(ui, view));
                    }
                    written.message = Some((now, selected));
                } else {
                    // One walk of the menu's texts a frame, for their fonts and sizes.
                    let texts = menus::texts(ui, menu);
                    let size = fonts.menu_measure(&menus::fonts_of(ui, &texts));
                    menus::write_text_sizes_of(ui, &texts, &mut written.sizes[0], &size);
                }
            }
            _ => written.message = None,
        }
        if message_open {
            written.warn_fallback(menus::MESSAGE_MENU, menus::message_status(ui));
        }
        if (switch.quantity || container_tiled) && prompt_open {
            written.warn_fallback(menus::QUANTITY_MENU, menus::quantity_built(ui));
        }
        match prompt {
            Some((menu, q)) => {
                let amount = menus::Amount { amount: q.amount, most: q.max };
                if written.quantity_text_scale!=Some(text_scale) {
                    written.quantity_text_scale=Some(text_scale);written.sizes[1].clear();
                }
                if written.quantity != Some((menu, amount)) {
                    menus::write_quantity(ui, amount);
                    // The amount's text follows from the meter: worked out before its size.
                    ui.update();
                }
                let texts = menus::texts(ui, menu);
                let size = fonts.menu_measure(&menus::fonts_of(ui, &texts));
                menus::write_text_sizes_of(ui,&texts,&mut written.sizes[1],&|font,text,wrap|
                    crate::menu::readability::measured(&size,text_scale,font,text,wrap));
                if written.quantity != Some((menu, amount)) {
                    info!("{}", menus::describe_quantity(ui));
                    written.quantity = Some((menu, amount));
                }
            }
            None => written.quantity = None,
        }
    }

    /// T15b: write the Wait / Sleep menu's traits (`wait_menu::write`) when what it shows
    /// changed, and its text tiles' sizes every frame it is drawn, before scripts.
    fn write_wait(&mut self, game: &mut fnv_game::Game<'_>, switch: vanilla::TileMenus, fonts: &mut draw::FontKit, calendar: Option<fnv_sim::clock::Calendar>) {
        let written = &mut *self.written;
        let open = switch.sleep_wait && game.menu == Some(crate::script::MENU_SLEEP_WAIT);
        if open {
            written.warn_fallback(wait_menu::SLEEP_WAIT_MENU, wait_menu::built(&game.ui_tree));
        }
        let model = self.wait.as_ref().and_then(|w| w.0.as_ref());
        let (Some(model), Some(words), Some(cal), true) = (model, self.words.as_ref(), calendar, open && wait_menu::tiled(true, game)) else {
            written.sleep_wait = None;
            return;
        };
        let Some(menu) = game.ui_tree.menu(wait_menu::SLEEP_WAIT_MENU) else { return };
        let now = wait_menu::Shown { menu, view: model.view(words, &cal) };
        let ui = &mut game.ui_tree;
        let changed = written.sleep_wait.as_ref() != Some(&now);
        if changed {
            wait_menu::write(ui, &now.view);
            ui.update();
        }
        let texts = menus::texts(ui, menu);
        let size = fonts.menu_measure(&menus::fonts_of(ui, &texts));
        menus::write_text_sizes_of(ui, &texts, &mut written.sizes[2], &size);
        if changed {
            info!("{}", wait_menu::describe(ui));
            written.sleep_wait = Some(now);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write(
    mut script: ResMut<ScriptState>,
    files: MenuFiles,
    hud: HudTiles,
    mut fonts: draw::FontKit,
    menus: Res<vanilla::TileMenus>,
    time: Res<Time>,
    mut animation: Local<fnv_game::ui_hud::Animation>,
    mut told: Local<[Option<String>; 4]>,
    mut menu_tiles: MenuTiles,
    mut mods_entry: ResMut<crate::pause::ModsEntry>,
    mut start_names: Local<mods::KeptNames>,
) {
    let Some(rt) = script.rt.as_mut() else { return };
    let before = counts_on().then(|| rt.game.ui_tree.trait_write_counts());
    // T15a: the prompt over the container, barter and inventory menus builds `QuantityMenu`
    // while it shows (`Game::ui_prompt`), whether or not it is drawn from it, as the open
    // menus build (T3b).
    rt.game.ui_prompt = menu_tiles.quantity().map(|_| fnv_game::game::menus::QUANTITY);
    {
        let _span = info_span!("tiles::write::sync").entered();
        files.write(&mut rt.game);
    }
    let menus_span = info_span!("tiles::write::menus").entered();
    // T11: the pause column's Mods entry, while a mod's tiles are in the start menu.
    let ui_files = rt.game.ui_files.as_deref().map(|f| f as &dyn fnv_ui::Source);
    let mods_now = crate::pause::ModsEntry(mods::available(*menus, &rt.game.ui_tree, ui_files, &mut start_names));
    if *mods_entry != mods_now {
        info!("tiles: {}", if mods_now.0 { "a mod's tiles in StartMenu: the pause menu has a Mods entry" } else { "no Mods entry in the pause menu" });
        *mods_entry = mods_now;
    }
    menu_tiles.write(&mut rt.game, *menus, &mut fonts, hud.sim.world.calendar().ok());
    drop(menus_span);
    let shown = {
        let _span = info_span!("tiles::write::shown").entered();
        hud.shown(&rt.game, &files)
    };
    let mut texts = {
        let _span = info_span!("tiles::write::texts").entered();
        hud.texts(&mut rt.game, shown)
    };
    let animation_span = info_span!("tiles::write::animation").entered();
    if let Some(root) = rt.game.ui_tree.find_tile("HUDMainMenu") {
        // [design] Reset clocks on tree rebuild/save-load, without touching the hand HUD.
        if rt.game.ui_tree.tile(root).get("_NVFAnimationClock").is_none() {
            *animation = Default::default();
            rt.game.ui_tree.set_trait(root, "_NVFAnimationClock", Value::Num(1.0));
        }
        texts.frame.animation = animation.step(f64::from(time.delta_secs()), shown.root && shown.corners, fnv_game::experience::standing(&rt.game), texts.frame.crippled);
    }
    drop(animation_span);
    // Who reads the HUD's traits: the tile-drawn HUD, mod tiles in it (their traits may
    // copy the HUD's), or a script that called a UI function. With none (the default
    // game), the writes wait for the first UI call (`ui_hud::flush`) and the frame does
    // no tree work [design: maintainer's review of #902]. The widths are kept back with
    // the strings (their measure), so the first UI call reads them as a frame writes
    // them. The corners are written only with the tile-drawn HUD, and nothing waits then.
    let ui = &rt.game.ui_tree;
    let mod_tiles = ui.menu(draw::HUD_MENU).is_some_and(|m| ui.has_mod_added_tiles(m));
    if !(menus.hud || mod_tiles || rt.game.ui_used) {
        let widths = {
            let _span = info_span!("tiles::write::widths").entered();
            if rt.game.ui_files.is_some() { fonts.widths(&rt.game.ui_tree) } else { None }
        };
        fnv_game::ui_hud::defer(&mut rt.game, shown, texts, widths);
        tell_counts(before, &rt.game, &fonts, true);
        return;
    }
    rt.game.ui_hud_pending = None;
    {
        let _span = info_span!("tiles::write::hud_write").entered();
        fnv_game::ui_hud::write(&mut rt.game.ui_tree, shown, &texts);
    }
    // The text tiles' widths from the strings just written, before scripts read them
    // [design: ui-tile-tree.md §4.1; observed widths: hud.md §14.1].
    if rt.game.ui_files.is_some() {
        {
            let _span = info_span!("tiles::write::text_widths").entered();
            fonts.write_widths(&mut rt.game.ui_tree);
        }
        if menus.hud {
            {
                let _span = info_span!("tiles::write::corners").entered();
                let corner = hud.corner(shown, &mut fonts);
                vanilla::write_corners(&mut rt.game.ui_tree, &corner);
            }
            let notices_span = info_span!("tiles::write::notices").entered();
            // The messages, quest notices and subtitles, only while the switch is on: off,
            // nothing is made under their pieces [design].
            let shows = hud.notices(&rt.game, shown);
            notices::write(&mut rt.game.ui_tree, &shows, &fonts.hud_lines());
            // Their widths from the strings just written, before scripts read them, as
            // the other HUD texts' above.
            fonts.write_notice_widths(&mut rt.game.ui_tree);
            for (line, last) in notices::describe(&shows).into_iter().zip(told.iter_mut()) {
                if line.is_some() && line != *last {
                    info!("{}", line.as_deref().unwrap_or_default());
                }
                *last = line;
            }
            drop(notices_span);
            let _span = info_span!("tiles::write::breath").entered();
            // The breath meter: shown under the hand-built meter's rule, at the breath used
            // (`vanilla::write_breath`, hud.md §14.12).
            let used = hud.breath.as_ref().map_or(0.0, |b| 1.0 - f64::from(b.share()));
            vanilla::write_breath(&mut rt.game.ui_tree, shown.breath, used);
        }
    }
    tell_counts(before, &rt.game, &fonts, false);
}

/// Whether `NVF_PLAY_TEST_TILE_COUNTS` asks for the frame's counts (the module's notes).
fn counts_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PLAY_TEST_TILE_COUNTS").is_ok_and(|v| !v.trim().is_empty() && v.trim() != "0"))
}

/// The frame's counts line, from the tree's trait counts when the frame began (`None`
/// without the test aid: nothing is read or logged here).
fn tell_counts(before: Option<(u64, u64)>, game: &fnv_game::Game<'_>, fonts: &draw::FontKit, deferred: bool) {
    let Some((writes, changes)) = before else { return };
    let (writes_now, changes_now) = game.ui_tree.trait_write_counts();
    let candidates = if deferred || game.ui_files.is_none() { 0 } else { fnv_game::ui_hud::measured_texts(&game.ui_tree).len() };
    info!(
        "tile counts: writes={} changed={} speech={} faces={} measure_candidates={candidates} deferred={deferred}",
        writes_now - writes,
        changes_now - changes,
        game.speech.len(),
        fonts.faces_made(),
    );
}

/// T15b: what the tutorial menu drawn from tiles was last written from.
#[derive(Default)]
struct TutorialWritten {
    shown: Option<tutorial_menu::Shown>,
    sizes: menus::SizesWritten,
    /// The symbol's picture's width, read once.
    symbol: Option<Option<f64>>,
    /// [design] The fallback's reason is warned once per session.
    warned: bool,
}

/// T15b: the scroll written for the tutorial menu this frame, and what was last seen drawn.
#[derive(Resource, Default)]
struct TutorialScrollCheck {
    wrote: Option<f64>,
    drawn: Option<(f64, f64)>,
}

/// T15b: write the tutorial menu's traits after its keys and clicks were applied
/// (`crate::tutorial_menu::keys`), before the scripts: the page when it changed, then its
/// sizes, range and scroll every frame it is drawn (`tutorial_menu::settle`), so the frame's
/// page and scroll reach the tree together (`tutorial_menu`).
fn write_tutorial(
    mut script: ResMut<ScriptState>,
    switch: Res<vanilla::TileMenus>,
    mut view: ResMut<crate::tutorial_menu::TutorialView>,
    mut fonts: draw::FontKit,
    mut written: Local<TutorialWritten>,
    mut check: ResMut<TutorialScrollCheck>,
    bindings: Option<Res<crate::bindings::Bindings>>,
) {
    check.wrote = None;
    if !switch.tutorial {
        return;
    }
    let Some(rt) = script.rt.as_mut() else { return };
    let game = &mut rt.game;
    let open = game.ui_files.is_some() && game.menu == Some(fnv_game::game::menus::TUTORIAL);
    if let (true, false, menus::Tiled::No(reason)) = (open, written.warned, tutorial_menu::built(&game.ui_tree)) {
        warn!("{} drawn by hand: {reason}", tutorial_menu::TUTORIAL_MENU);
        written.warned = true;
    }
    if !tutorial_menu::tiled(true, game) {
        written.shown = None;
        check.drawn = None;
        return;
    }
    let pages = crate::tutorial_menu::pages_with(game, tutorial_menu::body_text, bindings.as_deref());
    let (Some(menu), false) = (game.ui_tree.menu(tutorial_menu::TUTORIAL_MENU), pages.is_empty()) else { return };
    let at = view.page.min(pages.len() - 1);
    let now = tutorial_menu::Shown { menu, page: pages[at].clone(), at, of: pages.len() };
    let changed = written.shown.as_ref() != Some(&now);
    let ui = &mut game.ui_tree;
    if changed {
        tutorial_menu::write(ui, &now);
        let file = tutorial_menu::symbol_file(ui);
        let texels = *written.symbol.get_or_insert_with(|| file.and_then(|f| fonts.picture_size(&f)).map(|s| f64::from(s.x)));
        tutorial_menu::write_symbol(ui, texels);
    }
    let texts = menus::texts(ui, menu);
    // Each round's fonts as the tree stands then: a computed font follows what was just written.
    let mut write_sizes = |ui: &mut fnv_ui::Ui, sizes: &mut menus::SizesWritten| {
        let size = fonts.menu_measure(&menus::fonts_of(ui, &texts));
        menus::write_text_sizes_of(ui, &texts, sizes, &size)
    };
    let scroll = tutorial_menu::settle(ui, &mut written.sizes, &mut write_sizes, &mut view);
    if changed {
        info!("{}", tutorial_menu::describe(ui, &now));
        written.shown = Some(now);
    }
    check.wrote = Some(scroll);
}

/// T15b: the scroll and the body's place once the tree is worked out after scripts (what is
/// drawn), logged when they change, and when the scroll is not the value written this frame
/// (smoke reads both: the model's value must outlast a click's pulse, as the wait menu's).
/// Its `clipwindow`s are turned on here, after the scripts and before the drawing.
fn check_tutorial_drawn(ui: &mut fnv_ui::Ui, check: &mut TutorialScrollCheck) {
    use tutorial_menu::{marker_y, scroll_bound, scroll_shown, text_y, TUTORIAL_MENU};
    let Some(wrote) = check.wrote.take() else { return };
    tutorial_menu::mark_windows(ui);
    let (Some(v), Some(y)) = (scroll_shown(ui), text_y(ui)) else { return };
    let n = |x: f64| Value::Num(x).text();
    if check.drawn != Some((v, y)) {
        info!("tiles: {TUTORIAL_MENU} scroll drawn at {} of {}, text y {}, marker y {}", n(v), n(scroll_bound(ui)), n(y), n(marker_y(ui).unwrap_or(-1.0)));
        check.drawn = Some((v, y));
    }
    if wrote != v {
        info!("tiles: {TUTORIAL_MENU} scroll drawn at {}, not the {} written", n(v), n(wrote));
    }
}

/// T15c: write the container menu's traits before the input's step (`input::feed`): a new
/// snapshot of the model when it changed, then its sizes, places, scroll and highlight every
/// frame it is drawn (`container_menu::settle`), so the input and the drawing see one state.
#[allow(clippy::too_many_arguments)]
fn write_container(
    mut script: ResMut<ScriptState>,
    switch: Res<vanilla::TileMenus>,
    active: Res<crate::menu::ActiveMenu>,
    mut sim: ResMut<crate::sim::SimState>,
    (combat, play): (Option<Res<crate::combat::Combat>>, Option<Res<crate::PlayWorld>>),
    mut forms: Local<Option<crate::pipboy::stats::read::Forms>>,
    mut fonts: draw::FontKit,
    mut st: ResMut<container_menu::ContainerTiles>,
) {
    if !switch.container {
        return;
    }
    let model = active.0.as_ref().filter(|_| container_menu::is_container(active.0.as_ref()));
    let Some(rt) = script.rt.as_ref() else { return };
    let game = &rt.game;
    if model.is_some() && game.ui_files.is_some() && game.menu == Some(crate::script::MENU_CONTAINER) && !st.warned {
        if let menus::Tiled::No(reason) = container_menu::built(&game.ui_tree) {
            warn!("{} drawn by hand: {reason}", container_menu::CONTAINER_MENU);
            st.warned = true;
        }
    }
    let Some(m) = model else {
        if st.menu.is_some() || st.shown().is_some() {
            st.close();
        }
        return;
    };
    let menu = if container_menu::tiled(true, game, true) { game.ui_tree.menu(container_menu::CONTAINER_MENU) } else { None };
    st.menu = menu;
    let Some(menu) = menu else { return };
    // What the model shows is read when it changed: an aid's figures run its conditions.
    let due = active.is_changed() || st.needs_snapshot(menu);
    // The load order's setting over NVForever's label.
    // With a controller the button that takes all (X, `crate::pad::menu_buttons`) [#1662].
    let hint = if due { crate::pad::game_hint(&fnv_game::ui_loading::game_string(game, container_menu::TAKE_ALL_HINT), &[KeyCode::KeyR], crate::pad::at_menu(crate::script::MENU_CONTAINER)) } else { String::new() };
    let new = due.then(|| {
        if forms.is_none() {
            *forms = play.as_ref().map(|p| crate::pipboy::stats::read::Forms::new(p.lo, p.schema));
        }
        let note = crate::menu::note(m, &sim);
        let info = m.selected_row(m.focus).map(|r| crate::pipboy::items::info_of(r, combat.as_deref(), sim.bypass_change_detection(), forms.as_mut(), Some(script.bypass_change_detection())));
        container_menu::Snapshot::of(m, info.as_ref(), note, hint)
    });
    let Some(rt) = script.rt.as_mut() else { return };
    let ui = &mut rt.game.ui_tree;
    let mut told = false;
    if let Some(s) = new {
        if st.is_new(menu, &s) {
            container_menu::write(ui, &s, &mut st);
            told = true;
        }
    }
    let texts = menus::texts(ui, menu);
    // Each round's fonts as the tree stands then: a computed font follows what was just written.
    let mut write_sizes = |ui: &mut fnv_ui::Ui, sizes: &mut menus::SizesWritten| {
        let size = fonts.menu_measure(&menus::fonts_of(ui, &texts));
        menus::write_text_sizes_of(ui, &texts, sizes, &size)
    };
    st.settle_shown(ui, &mut write_sizes);
    if let (true, Some(s)) = (told, st.shown()) {
        // New rows are marked before this frame's input (the clip) too.
        container_menu::mark_tiles(ui);
        info!("{}", container_menu::describe(ui, s));
    }
}

/// Works the tree out after the scripts; nothing to do on a frame where nothing changed
/// it (`fnv_ui::Ui::update`). The scripts' gradual changes step first, on real time
/// (`fnv_game::ui_gradients`, task T9).
#[allow(clippy::too_many_arguments)]
fn update(mut script: ResMut<ScriptState>, time: Res<Time<Real>>, mut wait_check: ResMut<input::WaitSliderCheck>, mut tutorial_check: ResMut<TutorialScrollCheck>, mut container: ResMut<container_menu::ContainerTiles>, mut barter: ResMut<barter_menu::BarterTiles>, mut recipe: ResMut<recipe_menu::RecipeTiles>, mut traits: ResMut<trait_menu::TraitTiles>, mut tags: ResMut<tag_menu::TagTiles>) {
    let Some(rt) = script.rt.as_mut() else { return };
    rt.game.ui_gradients.step(&mut rt.game.ui_tree, f64::from(time.delta_secs()));
    if rt.game.ui_files.is_some() {
        rt.game.ui_tree.update();
        input::check_wait_drawn(&rt.game.ui_tree, &mut wait_check);
        check_tutorial_drawn(&mut rt.game.ui_tree, &mut tutorial_check);
        // T15c: the marks after the scripts (a tile injected this frame is covered), and the
        // scroll drawn.
        if container.menu.is_some() {
            container_menu::mark_tiles(&mut rt.game.ui_tree);
            container.check_drawn(&rt.game.ui_tree, &mut |line| info!("{line}"));
        }
        if barter.menu().is_some() {
            barter_menu::mark_tiles(&mut rt.game.ui_tree);
            barter.view.check_drawn(&rt.game.ui_tree, &mut |line| info!("{line}"));
        }
        if recipe.menu.is_some() {
            recipe_menu::mark_tiles(&mut rt.game.ui_tree);
            recipe.check_drawn(&rt.game.ui_tree, &mut |line| info!("{line}"));
        }
        if traits.menu.is_some() {
            trait_menu::mark_tiles(&mut rt.game.ui_tree);
            traits.check_drawn(&rt.game.ui_tree, &mut |line| info!("{line}"));
        }
        if tags.menu.is_some() {
            selection_menu::mark_tiles::<{ fnv_game::game::menus::TAG_SKILLS }>(&mut rt.game.ui_tree);
            tags.check_drawn(&rt.game.ui_tree, &mut |line| info!("{line}"));
        }
        // Once, if something changes the tree's shape every frame (#1061).
        if let Some(line) = rt.game.ui_tree.take_reshape_warning() {
            warn!("{line}");
        }
    }
    fnv_game::ui_more::prune_attachments(&mut rt.game);
    // MenuTapKey's keys stay queued unless a menu takes tile input (`input::feed`) [design].
}

pub struct TilesPlugin;

impl Plugin for TilesPlugin {
    fn build(&self, app: &mut App) {
        // Writes run after physics, before activation's scripts and PlaySet::Scripts;
        // evaluation runs after PlaySet::Scripts, before the menus draw [design: ui-tile-tree.md §4.1].
        // The mod tiles draw with the menus, after evaluation [design: §4.1 step 4].
        // T12a: `[UI] TileMenus=` read at start; the hand-built pieces the tiles draw are
        // hidden after every drawing system, before visibility is passed down [design].
        vanilla::setup(app);
        input::setup(app);
        stats_menu::setup(app);
        inventory_menu::setup(app);
        map_menu::setup(app);
        vats_menu::setup(app);
        app.init_resource::<draw::TileFonts>()
            .init_resource::<runtime_images::RuntimeImages>()
            .init_resource::<local_map_service::LocalMapService>()
            .init_resource::<projected_ui::ProjectedUiTargets>()
            .init_resource::<TutorialScrollCheck>()
            .init_resource::<container_menu::ContainerTiles>()
            .init_resource::<barter_menu::BarterTiles>()
            .init_resource::<recipe_menu::RecipeTiles>()
            .init_resource::<repair_services_menu::RepairServicesTiles>()
            .init_resource::<repair_menu::RepairTiles>()
            .init_resource::<item_mod_menu::ItemModTiles>()
            .add_message::<crate::repair::RepairServicesInput>()
            .init_resource::<container_menu::TileWheelTitle>()
            .init_resource::<tag_menu::TagTiles>()
            .insert_resource(tag_menu::aid_from_env())
            .init_resource::<trait_menu::TraitTiles>()
            .insert_resource(trait_menu::aid_from_env())
            .init_resource::<menus::TileMenuBlur>()
            .init_resource::<dialog_menu::DialogTiles>()
            .init_resource::<dialog_menu::SharedWritten>()
            // T15b: after the tutorial menu's keys, in the scripts' chain, before they run.
            .add_systems(Update, write_tutorial.in_set(PlaySet::Scripts).after(crate::tutorial_menu::keys).before(crate::script::run_scripts))
            .add_systems(Update, write.after(PlaySet::Physics).after(crate::compass::place).before(PlaySet::Activation))
            // T15c: the container menu's state for this frame's input.
            .add_systems(Update, write_container.after(write).before(input::feed))
            .add_systems(Update, barter_menu::write_system.after(write).before(input::feed))
            .add_systems(Update, recipe_menu::write_system.after(write).before(input::feed))
            .add_systems(Update, repair_services_menu::write_system.after(write).before(input::feed))
            // [design] The trait and tag-skill lists, likewise.
            .add_systems(Update, trait_menu::write_system.after(write).before(input::feed))
            .add_systems(Update, selection_menu::write_system::<{ fnv_game::game::menus::TAG_SKILLS }>.after(write).before(input::feed))
            // T8: the pointer and keys after the engine's writes, before scripts (§4.1 step 1).
            .add_systems(Update, input::feed.after(write).after(crate::menu::WheelHoldSet).before(PlaySet::Activation))
            .add_systems(Update, global_image_metrics::update_globals.after(PlaySet::Scripts).before(update))
            .add_systems(Update, local_map_service::system.after(PlaySet::Scripts).before(update))
            .add_systems(Update, update.after(PlaySet::Scripts).before(PlaySet::Ui))
            .add_systems(Update, projected_ui::reconcile.after(update).before(PlaySet::Ui))
            .add_systems(Update, repair_services_menu::after_update.after(update).before(PlaySet::Ui))
            // The Pip-Boy's STATS page: written after the tile input and before the scripts;
            // again after its keys and decided before the frame and the tile drawing read it.
            .add_systems(Update, stats_menu::feed.after(input::feed).after(crate::pipboy::PointerSet).before(PlaySet::Activation))
            .add_systems(Update, stats_menu::write.after(stats_menu::feed).before(PlaySet::Activation))
            .add_systems(Update, stats_menu::settle.in_set(PlaySet::Ui).after(crate::pipboy::stats::prepare).before(crate::pipboy::frame::draw))
            // The Pip-Boy's ITEMS page, in the same three steps; its model's keys and commands
            // have run by `prepare`, the first system after them in the Pip-Boy's chain.
            .add_systems(Update, inventory_menu::feed.after(input::feed).after(crate::pipboy::PointerSet).before(PlaySet::Activation))
            .add_systems(Update, inventory_menu::write.after(inventory_menu::feed).before(PlaySet::Activation))
            .add_systems(Update, inventory_menu::settle.in_set(PlaySet::Ui).after(crate::pipboy::stats::prepare).before(crate::pipboy::frame::draw))
            // The Pip-Boy's DATA page, in the same three steps; its preparation (the model's keys,
            // the acts, what it shows) is `data::prepare`, before the decision and the frame.
            .add_systems(Update, map_menu::feed.after(input::feed).after(crate::pipboy::PointerSet).before(PlaySet::Activation))
            .add_systems(Update, map_menu::write.after(map_menu::feed).before(PlaySet::Activation))
            // The Local Map's outline as its runtime picture, before the decision writes its name.
            .add_systems(Update, map_menu::give_outline.in_set(PlaySet::Ui).after(crate::pipboy::data::prepare).before(map_menu::settle))
            .add_systems(Update, map_menu::settle.in_set(PlaySet::Ui).after(crate::pipboy::data::prepare).before(crate::pipboy::frame::draw))
            // The Pip-Boy's repair page, in the same three steps; its model's keys have run by
            // `prepare`, in the Pip-Boy's chain.
            .add_systems(Update, repair_menu::feed.after(input::feed).after(crate::pipboy::PointerSet).before(PlaySet::Activation))
            .add_systems(Update, repair_menu::write_system.after(repair_menu::feed).before(PlaySet::Activation))
            .add_systems(Update, repair_menu::settle.in_set(PlaySet::Ui).after(crate::pipboy::stats::prepare).before(crate::pipboy::frame::draw))
            // The Pip-Boy's weapon mod menu, in the same three steps; its model's keys have run
            // by `prepare`, in the Pip-Boy's chain.
            .add_systems(Update, item_mod_menu::feed.after(input::feed).after(crate::pipboy::PointerSet).before(PlaySet::Activation))
            .add_systems(Update, item_mod_menu::write_system.after(item_mod_menu::feed).before(PlaySet::Activation))
            .add_systems(Update, item_mod_menu::settle.in_set(PlaySet::Ui).after(crate::pipboy::stats::prepare).before(crate::pipboy::frame::draw))
            // The dialogue menu (`dialog_menu`): what the model says, at each of the runtime's
            // transitions and before the frame's scripts; then what they left measured and
            // placed, worked out by `update`, and what can be hit as it is then drawn.
            .add_systems(Update, dialog_menu::install_hook.after(PlaySet::Physics).before(PlaySet::Activation))
            .add_systems(Update, dialog_menu::publish_system.in_set(PlaySet::Scripts).after(crate::dialogue::input).before(crate::script::run_scripts))
            .add_systems(Update, dialog_menu::write_system.after(PlaySet::Scripts).before(update))
            .add_systems(Update, dialog_menu::snapshot_system.after(update).before(PlaySet::Ui))
            // The classic VATS screen (`vats_menu`): after the mode's work for the frame and
            // the scripts, built, written and worked out by `update` before it is drawn.
            .add_systems(Update, vats_menu::write_system.after(PlaySet::Scripts).after(crate::vats_mode::VatsUpdate).before(update))
            // After the character menus' sync, so the trait menu's decision reads this frame's.
            .add_systems(Update, (draw::draw, draw::scroll, model::animate).chain().in_set(PlaySet::Ui).after(crate::pipboy::frame::draw).after(crate::chargen::ChargenSet))
            .add_systems(Update, projected_ui::draw.in_set(PlaySet::Ui).after(draw::draw))
            .add_systems(PostUpdate, vanilla::hide_hand_built.before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));
    }
}

/// Lean authored-input Apps still use the real production writer, including its
/// prompt lifecycle publication, native amount traits and text measurement.
#[cfg(test)]
pub(crate) fn install_quantity_writer_for_touch_tests(app:&mut App) {
    app.init_resource::<draw::TileFonts>().init_resource::<crate::pause::ModsEntry>()
        .add_systems(Update,write.before(crate::menu::normal_touch::Dispatch));
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_ui::{MemSource, Ui};

    #[test]
    fn string_metrics_install_when_source_arrives_and_again_after_restore() {
        fn install(files: MenuFiles, mut scripts: ResMut<ScriptState>) {
            files.install_measure(&mut scripts.rt.as_mut().unwrap().game);
        }
        let mut app = App::new();
        app.insert_resource(ScriptState::over_no_plugins()).add_systems(Update, install);
        app.update();
        assert!(app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_extra.measure.is_none());

        // A synthetic, empty source: installation must not depend on GPU assets or a HUD.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target-check").join(format!("string-metrics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let schema = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif");
        app.insert_resource(GameData::open_for_plugins(&dir, &[] as &[&str], &schema).unwrap());
        app.update();
        {
            let mut scripts = app.world_mut().resource_mut::<ScriptState>();
            let game = &mut scripts.rt.as_mut().unwrap().game;
            assert!(game.ui_extra.measure.is_some());
            assert_eq!(game.ui_extra.measure.as_ref().unwrap()(9.0, "ab", 0.0), (0.0, 0.0, 0.0));
            // A later frame preserves the installed callback rather than replacing its cache.
            game.ui_extra.measure = Some(Box::new(|_, _, _| (12.0, 34.0, 1.0)));
        }
        app.update();
        {
            let mut scripts = app.world_mut().resource_mut::<ScriptState>();
            let game = &mut scripts.rt.as_mut().unwrap().game;
            assert_eq!(game.ui_extra.measure.as_ref().unwrap()(9.0, "ab", 0.0), (12.0, 34.0, 1.0));
            let snapshot = game.snapshot();
            let world = fnv_sim::World::restore(game.world.forms, game.world.placed, &snapshot.sim).unwrap();
            *game = fnv_game::Game::restore(game.data, world, game.cfg, &snapshot).unwrap();
            assert!(game.ui_extra.measure.is_none());
        }
        // This is the same installation used before the restart/load script runners.
        app.update();
        let scripts = app.world().resource::<ScriptState>();
        let game = &scripts.rt.as_ref().unwrap().game;
        assert_eq!(game.ui_extra.measure.as_ref().unwrap()(9.0, "ab", 0.0), (0.0, 0.0, 0.0));
        drop(app);
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn fallback_reasons_are_warned_once_across_reopens() {
        // [design] Reopening the same broken file or an oversized box cannot spam logs;
        // a different reason or menu still gets its own warning.
        let mut written = MenusWritten::default();
        for _ in 0..2 {
            written.warn_fallback(menus::MESSAGE_MENU, menus::Tiled::No("path missing".into()));
            written.warn_fallback(menus::MESSAGE_MENU, menus::Tiled::Yes);
        }
        assert_eq!(written.fallbacks.len(), 1);
        written.warn_fallback(menus::QUANTITY_MENU, menus::Tiled::No("path missing".into()));
        written.warn_fallback(menus::MESSAGE_MENU, menus::Tiled::No("button rows exceed LIST_MOST (420); scrolling is not built".into()));
        assert_eq!(written.fallbacks.len(), 3);
    }

    /// [design] A tile placed by `screen()`, as the T15 menus centre their boxes.
    fn centred() -> Ui {
        let src = MemSource::default().with(
            "m.xml",
            r#"<menu name="M"><rect name="Box"><width>100</width>
              <x><copy src="screen()" trait="width"/><sub src="me()" trait="width"/><div>2</div></x>
              <_thick><copy>2</copy><mul src="screen()" trait="resolutionconverter"/></_thick></rect></menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "m.xml").unwrap();
        ui
    }

    /// [design] The gate is global: `StatsMenu` beside the tile HUD changes what the HUD gets,
    /// the screen's units and whether a hotrect's `alpha` 0 passes to what is under it, and
    /// beside another menu of the gate it changes nothing more. A synthetic hotrect and image.
    #[test]
    fn stats_menu_turns_on_the_one_gate_every_menu_shares() {
        use vanilla::TileMenus;
        let hud = TileMenus { hud: true, ..Default::default() };
        let stats = TileMenus { stats: true, ..Default::default() };
        let both = TileMenus { hud: true, stats: true, ..Default::default() };
        assert!(!interface_screen(hud));
        assert!(interface_screen(stats));
        assert!(interface_screen(both), "the HUD beside it gets interface units too");
        let message = TileMenus { message: true, ..Default::default() };
        assert_eq!(interface_screen(message), interface_screen(TileMenus { stats: true, ..message }), "already on: no further change");
        let src = MemSource::default().with("m.xml", r#"<menu name="M"><hotrect name="H"><alpha>0</alpha><image name="I"><width>4</width><height>4</height><filename>a.dds</filename></image></hotrect></menu>"#);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "m.xml").unwrap();
        ui.update();
        let drawn = |menus: TileMenus| {
            let measure = |_: f64, _: &str, _: Option<f64>| (0.0, 0.0);
            draw::pieces(&ui, menu, &measure, draw::Select::Whole { kept: &[], strip: None }, interface_screen(menus)).0.len()
        };
        assert_eq!(drawn(hud), 0, "the tile HUD alone keeps the hotrect's alpha");
        assert_eq!(drawn(both), 1, "beside StatsMenu the hotrect's alpha 0 passes");
        // The screen's traits are the window's interface units, never the Pip-Boy's canvas.
        write_screen(&mut ui, 1280.0, 720.0, interface_screen(both));
        ui.update();
        let s = ui.tile(ui.screen());
        assert_eq!(s.get("height").map(Value::num), Some(960.0));
        assert_eq!(s.get("resolutionconverter").map(Value::num), Some(960.0 / 720.0));
    }

    #[test]
    fn screen_keeps_the_windows_pixels_unless_a_t15_menu_is_on() {
        // Off (the default game, or the tile HUD alone): the window's size, no converter,
        // and a tile placed by screen() where it was before T15a.
        assert!(!interface_screen(vanilla::TileMenus::default()));
        assert!(!interface_screen(vanilla::TileMenus { hud: true, ..Default::default() }));
        let mut ui = centred();
        write_screen(&mut ui, 1280.0, 720.0, false);
        ui.update();
        let s = ui.tile(ui.screen());
        assert_eq!((s.get("width").map(Value::num), s.get("height").map(Value::num)), (Some(1280.0), Some(720.0)));
        assert!(s.get("resolutionconverter").is_none());
        assert_eq!(ui.get_float("M/Box/x"), Some(590.0));
        assert_eq!(ui.get_float("M/Box/_thick"), Some(0.0));
        // On: interface units, 960 high, and the converter.
        assert!(interface_screen(vanilla::TileMenus { message: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { quantity: true, ..Default::default() }));
        // The start menu's switch alone does not (on by default; its Mods page asks while up).
        assert!(!interface_screen(vanilla::TileMenus { start: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { sleep_wait: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { tutorial: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { container: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { barter: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { recipe: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { repair_services: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { repair_menu: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { item_mod: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { traits: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { tags: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { dialog: true, ..Default::default() }));
        assert!(interface_screen(vanilla::TileMenus { map: true, ..Default::default() }));
        write_screen(&mut ui, 1280.0, 720.0, true);
        ui.update();
        assert_eq!(ui.get_float("M/Box/x").map(|x| (x * 100.0).round() / 100.0), Some(803.33));
        // A line 2 pixels thick: 2 × 960/720 units.
        assert_eq!(ui.get_float("M/Box/_thick"), Some(2.0 * 960.0 / 720.0));
        assert_eq!(screen_size(1280.0, 720.0).1, 960.0);
    }
}

