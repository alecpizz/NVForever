//! Loading screens for travel (doors, scripts moving the player): the game holds still
//! under a full-screen picture until the destination is loaded
//! (docs/design/loading-screen.md; records: docs/spec-notes/loading-screens.md).
//!
//! - Whoever moves the player sends a [`TravelRequest`]. The screen comes up first
//!   ([`PlayState::Loading`]); [`SHOW_FRAMES`] frames later, once it has been drawn, the
//!   request goes on as the [`Travel`] that physics and the renderer apply, so their
//!   work happens behind the picture and not in front of a frozen view of the old place.
//! - While it is up nothing happens in the world: the systems that wait for
//!   `PlayState::Playing` stand still, scripts do not run (`script::run_scripts`), and
//!   Bevy's virtual clock is paused, so everything driven by `Time` (animations, doors,
//!   NPCs, the sky's clock, test aids) holds too. Streaming goes on: it is what is being
//!   waited for.
//! - It comes down when physics has every cell around the player
//!   (`PhysState::loading_done`), the renderer has drawn its cells (it reports through
//!   [`Loading::render_busy`]) and no actor in view still waits for its parts
//!   ([`Loading::actors_busy`]), [`SETTLE_SECS`] later so that textures and meshes reach
//!   the graphics card behind the picture. With ALC shadows on it also waits while their
//!   maps are drawn behind it ([`Loading::lamp_shadows_busy`]), and with Volumetric clouds on,
//!   for a second at most, until every part of their picture has been cast ([`Loading::clouds_busy`]).
//! - A run that starts in the world ([`StartCover`]) is held under the screen from its
//!   first frame until the same is in, the scripts are made (a save put back into
//!   them) and the residents in view are drawn ([`Loading::start_actors`], for at most
//!   [`PLACE_CAP_SECS`]); the test aids' clock and `--run-seconds` count from when it
//!   comes down (docs/design/loading-screen.md §1a).
//! - The picture and its text are a loading screen record of the plugins
//!   (`fnv_world::loadscreen`): one that belongs to the destination, or one for anywhere.
//!   As the original lays it out [observed: loading-screens.md §4]: the picture stretched
//!   to the window, the text at the top on its band and the turning emblem at the bottom
//!   (`crate::loading_look`). The original's picture stage, objective, stats and level bar
//!   are not built.
//! - A [`CoverRequest`] brings the same screen up with no trip: it holds the game as
//!   above and stays until an [`Uncover`]. nvforever puts it up while loading a save or
//!   going back to the main menu, which start the program again behind it
//!   (nvforever `handoff.rs`); [`Loading::unseen`] is that new run's own start while its
//!   window is still hidden, when streaming may take long frames as behind a screen.
//!   An [`Uncover`] goes back to what the game was in when the cover came: the main
//!   menu or the pause menu, still holding the game, after a load that came to nothing.
//! - A script's EnableLoadingMenu before it moves the player makes that travel's screen
//!   one for anywhere ([`Loading::generic_next`]). The original's few seconds of screen
//!   for the call alone, with no travel, are not built (no shipped call is alone).
//! - While it is up the player's keys, mouse buttons and controller do nothing
//!   ([`hold_input`]): a menu still open behind it (the main menu while a save loads)
//!   takes no clicks, hovers or arrow keys, and a key held through the screen counts
//!   only once it is pressed again.

use crate::interface::bc_supported;
use crate::loading_look::{self as look, Look};
use crate::phys::PhysState;
use crate::{Location, PlaySet, PlayState, PlayWorld, Travel};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use fnv_world::loadscreen::{choose_with, load_screens, Destination, LoadScreen, TipStyle};
use nvf_assets::{GameData, TextureCache};

/// Frames the screen is up before the travel is applied [engineering choice: the frame
/// it is spawned in and one more, so the picture is on screen before the long frames].
pub const SHOW_FRAMES: u32 = 2;

/// Real seconds (and at least [`SETTLE_FRAMES`] frames) the screen stays after the last
/// cell is in [engineering choice].
pub const SETTLE_SECS: f64 = 0.25;
pub const SETTLE_FRAMES: u32 = 3;

/// The most real seconds a settled screen stays up for the volumetric clouds
/// ([`Loading::clouds_busy`]), counted with the settle time from when everything else was in
/// [design: their nine casts take 0.15 s at 60 frames a second, inside the settle time, and fit
/// the limit down to about nine frames a second; a screen must never hang on them].
pub const CLOUDS_HOLD_SECS: f64 = 1.0;

/// Real seconds after which a loading screen comes down whatever is still missing
/// (logged as a warning), so a cell that never loads cannot hold the game forever.
pub const TIMEOUT_SECS: f64 = 120.0;
/// Real seconds a screen waits for pipelines alone once all else is in: a pipeline that
/// never comes must not hold the game for [`TIMEOUT_SECS`] [engineering choice: about
/// four times the longest wait measured, a Goodsprings start's 3.7 s].
pub const PIPELINE_CAP_SECS: f64 = 15.0;
/// Render frames the start's screen is given to reach the window, the pipelines it is
/// drawn with ready,
/// before the world starts loading behind it [engineering choice: the frame that asks for
/// them, the one that prepares the picture, the one that draws it].
pub const PICTURE_FRAMES: u32 = 3;
/// Real seconds the world waits for that at most [engineering choice: a start must not
/// depend on it; about twice the 2-2.9 s measured, and the scripts take about 5 s beside it].
pub const PICTURE_CAP_SECS: f64 = 5.0;
/// Real seconds the start's screen waits for its residents to be drawn under it
/// ([`StartActors`]), from the first frame they may be drawn ([`StartActors::drawing`]): then
/// it lets them go, and those left are drawn after the screen, a few a frame, as before
/// [engineering choice: a start
/// must never hang on them; the 15-17 bodies of a Novac start took about 1 s of frames
/// after the screen, observed 2026-11-19].
pub const PLACE_CAP_SECS: f64 = 3.0;

/// Move the player to another place behind a loading screen; see [`Travel`] for the
/// fields. Sent by activation and scripts; [`LoadingPlugin`] turns it into the `Travel`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct TravelRequest(pub Travel);

/// Bring a loading screen up with no travel (module notes): its picture is one for `to`
/// (or for anywhere without it); it stays until [`Uncover`].
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CoverRequest {
    pub to: Option<Destination>,
}

/// Take down the screen a [`CoverRequest`] put up, and let the game go on.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Uncover;

/// The trip a loading screen is up for.
#[derive(Debug, Clone, Copy)]
struct Trip {
    travel: Travel,
    /// The `Travel` has gone out.
    sent: bool,
    /// Frames since the screen came up.
    frames: u32,
    /// Real time the screen came up.
    started: f64,
    /// Since when everything has been loaded (real time, frames).
    ready: Option<(f64, u32)>,
}

/// Asked for by nvforever for a run that starts in the world (`--play`, not on the main
/// menu or behind the intro film): the start is held behind a loading screen
/// ([`Loading::starting`]) until the world can be played (module notes).
#[derive(Resource)]
pub struct StartCover;

/// The start held behind a loading screen.
#[derive(Debug, Clone, Copy)]
struct StartHold {
    /// Real time the screen came up.
    started: f64,
    /// Since when everything has been ready (real time, frames).
    ready: Option<(f64, u32)>,
}

/// The loading screen's state.
#[derive(Resource, Default)]
pub struct Loading {
    /// The player's tweak: no tip text or band (`crate::tweaks`).
    pub hide_tips: bool,
    /// The player's Click to Exit Load Screens tweak: a finished screen stays up, saying
    /// so, until a key or a button is pressed [design: docs/design/tweaks.md].
    pub click_to_exit: bool,
    /// The player's No Exterior Load Screens tweak: a trip out of an interior shows the
    /// black alone, no picture or tip [design: docs/design/tweaks.md].
    pub plain_going_out: bool,
    /// Named NVForever option `location-load-screens` (off by default; #3844): prefer
    /// location-specific load screens when any match the destination, synced live from
    /// [`LocationLoadScreens`] [documented: JIP LN NVSE v57.30 / Settings INI v56.24
    /// `jip_nvse.ini` `bLoadScreenFix`; design: `docs/spec-notes/location-load-screens.md`].
    pub prefer_location: bool,
    /// The next screen is the plain one (set for one [`spawn_screen`]).
    plain_once: bool,
    waiting_click: bool,
    trip: Option<Trip>,
    /// The start of the run, held until the world can be played ([`StartCover`]).
    start: Option<StartHold>,
    /// Actors around the player whose parts are still being read, so they are not drawn
    /// yet. Set by nvforever every frame (`readahead::actors_waiting`); stays false
    /// without it.
    pub actors_busy: bool,
    /// The residents in view drawn under the start's screen, and those still to draw. Set
    /// by nvforever every frame while the start's screen is up (`residents::drive`); stays
    /// `None` without residents.
    pub start_actors: Option<StartActors>,
    /// Since when the start's screen has waited for its residents while they may be drawn
    /// (real time), and whether [`PLACE_CAP_SECS`] has passed.
    placing_since: Option<f64>,
    placing_capped: bool,
    /// A screen is up for a [`CoverRequest`].
    cover: bool,
    /// What the game was in when a [`CoverRequest`] came (its state, and whether the
    /// pause menu held it): an [`Uncover`] goes back to it. Leaving `PlayState::Menu`
    /// for the screen closes the pause menu (`crate::pause`), so a load from the main
    /// menu or the pause menu that comes to nothing would otherwise go on playing, quest
    /// scripts and all, behind the menu.
    cover_from: Option<(PlayState, bool)>,
    /// Nothing of the game is on screen yet (nvforever `handoff.rs`: a start whose window
    /// is still hidden): loading may take long frames, as behind a screen.
    pub unseen: bool,
    /// The renderer still has cells to draw around the camera. Set by the renderer every
    /// frame after its streaming (nvforever `loading_gate`); stays false without one.
    pub render_busy: bool,
    /// Loading screens shown so far.
    pub shown: u32,
    /// The next travel's screen is one for anywhere, not one of its destination: a
    /// script called EnableLoadingMenu before moving the player (`crate::script`)
    /// [documented: GECK wiki EnableLoadingMenu, "does not use any loading screens
    /// specific to the current location"].
    pub generic_next: bool,
    /// The next travel is the player's own trip through a load door or a fast travel,
    /// not a script moving the player (MoveTo): the actors of the destination are put
    /// at their package's place behind its screen (nvforever `residents.rs`, issue
    /// #1208). Set by the door's activation and the fast travel; taken by the residents
    /// when the `Travel` goes out.
    pub arriving: bool,
    /// Pipelines the view still needs are compiling. Set by the renderer every frame
    /// (nvforever `loading_gate`, `pipeline_gate`); stays false without one.
    pub pipelines_busy: bool,
    /// Hidden Pip-Boy draws or their teardown still need the cover [design: nvforever/pipboy_arm.rs].
    pub pipboy_warming: bool,
    /// The lamps' shadow maps are still being drawn behind the screen, so they are whole when
    /// it comes down. Set by nvforever every frame (`light_shadows`, its pre-draw, which gives
    /// up after a few seconds of its own); stays false without it.
    pub lamp_shadows_busy: bool,
    /// The volumetric clouds are asked for but not yet drawn behind the screen with every texel
    /// cast since the last cut (nvforever `medium.rs` `clouds_gate`, set every frame). A settled
    /// screen waits for them up to [`CLOUDS_HOLD_SECS`]; stays false without them.
    pub clouds_busy: bool,
    /// They are so drawn, for the log as the screen comes down (false also without them).
    pub clouds_ready: bool,
    /// Since when only pipelines have been waited for (real time), and whether
    /// [`PIPELINE_CAP_SECS`] has passed this screen.
    pipelines_since: Option<f64>,
    pipelines_capped: bool,
    /// The cap has just passed: nvforever logs which pipelines are late.
    pub pipeline_cap_note: bool,
    /// The start's screen is not on the window yet (since this real time): the world holds
    /// off, so that its reads and pipelines do not come first ([`Loading::world_held`]).
    picture_hold: Option<std::time::Instant>,
    /// The screen that is up so far, timed as [`LoadPhases`].
    phases: LoadPhases,
    /// The last screen that came down, timed (issue #2407): nvforever's reports send it
    /// with a slow load.
    pub last_phases: Option<LoadPhases>,
}

/// The residents (nvforever `residents.rs`) under the start's screen: drawn behind it,
/// so that the first seconds of play do not spawn them a few a frame (docs/design/
/// loading-screen.md §1a).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StartActors {
    /// Residents drawn under the screen so far.
    pub placed: usize,
    /// Residents in view not drawn yet.
    pub left: usize,
    /// They are drawn under the screen, which waits for them; off with test aid
    /// `NVF_START_ACTORS_UNDER_SCREEN=0`, which draws them after it, as before.
    pub under_screen: bool,
    /// They may be drawn now (the cells around the player in, drawn and in physics, the
    /// scripts made). The screen waits for them before too, held by those, but
    /// [`PLACE_CAP_SECS`] counts only from here.
    pub drawing: bool,
}

/// Where a loading screen's time went (issue #2407): for each thing it waited for
/// ([`missing`]: cells to draw, cells in physics, actors to draw, actors to place,
/// pipelines, the scripts, the Pip-Boy warm-up), the real seconds from the screen coming up to the last frame it was
/// still missing, and the longest frame under the screen. A wait that ends last is what held
/// the screen; a long frame is work on the main thread (a cell built, a save read) where a
/// long wait made of short frames is work elsewhere (reads from the disk, the model and
/// texture pools, pipeline compiles) [engineering choice].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadPhases {
    /// Seconds the screen was up.
    pub total: f64,
    /// Each wait and when it ended, in the order first seen.
    pub until: Vec<(&'static str, f64)>,
    /// The longest frame under the screen (real seconds) and the frames counted.
    pub longest_frame: f64,
    pub frames: u32,
    /// What the game files gave under the screen: files, bytes, time in the reads and
    /// time queued for an archive (`fnv_vfs::read_stats`; from its first frame counted).
    pub reads: fnv_vfs::ReadStats,
    reads_from: Option<fnv_vfs::ReadStats>,
    /// The pipelines compiled under the screen and where their time went, summed over the
    /// compile threads (vendored bevy_render `nvf_pipeline_time`, #2407).
    pub pipelines: bevy::render::render_resource::NvfPipelineTime,
    pipelines_from: Option<bevy::render::render_resource::NvfPipelineTime>,
}

impl LoadPhases {
    /// Count a frame `at` seconds after the screen came up, `frame` long, still waiting for `wait`.
    fn note(&mut self, wait: &[&'static str], at: f64, frame: f64) {
        let now = fnv_vfs::read_stats();
        self.reads = now.since(self.reads_from.get_or_insert(now));
        let piped = bevy::render::render_resource::nvf_pipeline_time();
        self.pipelines = piped.since(self.pipelines_from.get_or_insert(piped));
        self.frames += 1;
        self.total = self.total.max(at);
        self.longest_frame = self.longest_frame.max(frame);
        for &w in wait {
            match self.until.iter_mut().find(|(n, _)| *n == w) {
                Some((_, t)) => *t = t.max(at),
                None => self.until.push((w, at)),
            }
        }
    }

    /// The log line's text: `up 3.40 s; cells to draw 3.10 s, pipelines 0.40 s; longest frame 0.90 s of 41; read 812 files, 95.0 MB, 0.30 s reading, 0.10 s queued for an archive; 45 pipelines compiled: 1.20 s queued for a thread, 0.30 s for the shader cache, 0.10 s composing, 0.40 s making modules, 2.00 s creating`
    /// (the pipelines' seconds are summed over the compile threads).
    pub fn text(&self) -> String {
        let waits = if self.until.is_empty() { "nothing waited for".to_string() } else { self.until.iter().map(|(n, t)| format!("{n} {t:.2} s")).collect::<Vec<_>>().join(", ") };
        let (r, p) = (&self.reads, &self.pipelines);
        format!(
            "up {:.2} s; {waits}; longest frame {:.2} s of {}; read {} files, {:.1} MB, {:.2} s reading, {:.2} s queued for an archive; {} pipelines compiled: {:.2} s queued for a thread, {:.2} s for the shader cache, {:.2} s composing, {:.2} s making modules, {:.2} s creating",
            self.total,
            self.longest_frame,
            self.frames,
            r.files,
            r.bytes as f64 / 1e6,
            r.read_ns as f64 / 1e9,
            r.queued_ns as f64 / 1e9,
            p.pipelines,
            p.queued_ms / 1e3,
            p.lock_ms / 1e3,
            p.compose_ms / 1e3,
            p.naga_ms / 1e3,
            p.create_ms / 1e3
        )
    }
}

impl Loading {
    /// Whether a loading screen is up.
    pub fn active(&self) -> bool {
        self.trip.is_some() || self.cover || self.start.is_some()
    }

    /// Whether the start's screen is up ([`StartCover`]). Its time counts on no test
    /// aid's clock (`crate::pause::PlayClock`) and not in `--run-seconds`: they count
    /// from when it comes down.
    pub fn starting(&self) -> bool {
        self.start.is_some()
    }

    /// Whether a loading screen is up for travel (not for a [`CoverRequest`]).
    pub fn travelling(&self) -> bool {
        self.trip.is_some()
    }

    /// Whether the start's or the travel's screen has everything in and only waits out
    /// [`SETTLE_SECS`] before it comes down (nvforever `pipeline_prewarm` stops drawing the
    /// cells unculled then, so the first frame seen is culled).
    pub fn settling(&self) -> bool {
        match (&self.trip, &self.start) {
            (Some(trip), _) => trip.sent && trip.ready.is_some(),
            (None, Some(start)) => start.ready.is_some(),
            (None, None) => false,
        }
    }

    /// Whether the world holds off loading until the start's screen has been drawn. The hold
    /// began when model reads shared the pool that compiles the screen's pipelines
    /// (docs/design/loading-screen.md §1a). They have their own pool now; whether the hold
    /// still helps a start has not been measured (#1732).
    pub fn world_held(&self) -> bool {
        self.picture_hold.is_some()
    }

    /// Let the world load once the screen is `drawn`, or [`PICTURE_CAP_SECS`] after it came up.
    ///
    /// By the wall clock: `Time<Real>` stands still for a whole frame, and the first frames
    /// are the long ones the cap is there to outlast.
    pub fn release_picture_hold(&mut self, drawn: bool, now: std::time::Instant) {
        let Some(since) = self.picture_hold else { return };
        let waited = now.duration_since(since).as_secs_f64();
        if drawn || waited >= PICTURE_CAP_SECS {
            self.picture_hold = None;
            info!("loading screen: the world starts loading {waited:.2} s after the screen ({})", if drawn { "drawn" } else { "not drawn yet" });
        }
    }

    /// Whether a long frame would not be seen: a screen is up, or nothing is shown yet.
    pub fn unseen_frames(&self) -> bool {
        self.active() || self.unseen
    }

    /// Whether the start's screen has stopped waiting for its residents
    /// ([`PLACE_CAP_SECS`]): those left are drawn after it.
    pub fn placing_capped(&self) -> bool {
        self.placing_capped
    }

    /// A start's screen that is up, and settling or not, for the tests of who follows it.
    #[cfg(feature = "test-support")]
    pub fn starting_for_test(settling: bool) -> Loading {
        Loading { start: Some(StartHold { started: 0.0, ready: settling.then_some((0.0, 0)) }), ..default() }
    }
}

/// The loading screens of the plugins.
#[derive(Resource)]
struct Screens(Vec<LoadScreen>);

#[derive(Component)]
struct LoadingRoot;

/// Named NVForever option `location-load-screens` (off by default; #3844): when enabled,
/// prefer location-specific `LSCR` loading screens matching the destination cell or
/// worldspace over generic screens [documented: JIP LN NVSE v57.30 / Settings INI v56.24
/// `jip_nvse.ini` `bLoadScreenFix`; no DLL or source consulted; design:
/// `docs/spec-notes/location-load-screens.md`].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocationLoadScreens(pub bool);

/// Keep [`Loading::prefer_location`] aligned with the live [`LocationLoadScreens`] setting
/// so toggling it from the pause menu takes effect immediately.
pub(crate) fn sync_location_load_screens(setting: Option<Res<LocationLoadScreens>>, mut loading: ResMut<Loading>) {
    let want = setting.as_deref().is_some_and(|s| s.0);
    if loading.prefer_location != want {
        loading.prefer_location = want;
    }
}

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Loading>()
            .init_resource::<LocationLoadScreens>()
            .init_resource::<Look>()
            .add_message::<TravelRequest>()
            .add_message::<CoverRequest>()
            .add_message::<Uncover>()
            .add_systems(Startup, read_screens)
            .add_systems(
                PreUpdate,
                hold_input.after(bevy::input::InputSystems).after(crate::pad::menu_keys).after(crate::test_aid::real_presses).before(bevy::ui::UiSystems::Focus),
            )
            .add_systems(Update, (send.in_set(PlaySet::Input), (sync_location_load_screens, begin_start, begin, hold, look::fit_tip, look::turn_emblem).chain().in_set(PlaySet::Sync)))
            .add_systems(Last, finish);
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "location-load-screens",
                group: "Interface",
                label: "Location-specific loading screens",
                note: "On: prefer loading screens matching the destination cell or worldspace over generic screens. Off: mix matching and generic loading screens as in the original.",
                switches: "",
                get: |w| w.resource::<LocationLoadScreens>().0,
                set: |w, on| w.resource_mut::<LocationLoadScreens>().0 = on,
                slider: None,
            },
        );
    }
}

/// While a screen is up nobody's input gets through (module notes): the keys and mouse
/// buttons are let go before any system or the interface reads them (the controller's
/// menu keys too, which `crate::pad` presses as keys). Test aids' presses are held the
/// same way.
pub(crate) fn hold_input(loading: Res<Loading>, keys: Option<ResMut<ButtonInput<KeyCode>>>, mouse: Option<ResMut<ButtonInput<MouseButton>>>) {
    if loading.active() {
        if let Some(mut k) = keys {
            k.reset_all();
        }
        if let Some(mut m) = mouse {
            m.reset_all();
        }
    }
}

fn read_screens(mut commands: Commands, d: Option<Res<PlayWorld>>) {
    let Some(d) = d else { return };
    let screens = load_screens(d.lo, d.schema);
    let placed = screens.iter().filter(|s| !s.locations.is_empty()).count();
    info!("loading screens: {} in the plugins, {placed} of them for particular places", screens.len());
    commands.insert_resource(Screens(screens));
}

/// The size and offset of a `picture` scaled to cover a `window` without changing its
/// shape: the overflow is cropped evenly on both sides. The main menu's and the credits'
/// pictures; a loading screen's is stretched ([`picture_node`]).
pub fn cover(picture: Vec2, window: Vec2) -> (Vec2, Vec2) {
    if picture.x <= 0.0 || picture.y <= 0.0 {
        return (window, Vec2::ZERO);
    }
    let size = picture * (window.x / picture.x).max(window.y / picture.y);
    (size, (window - size) / 2.0)
}

/// Where a travel leaving `from` arrives, for the choice of screen.
pub fn destination(t: &Travel, from: Location) -> Destination {
    let to = t.destination(from);
    Destination { interior: to.interior, exterior: to.interior.is_none().then(|| (to.world, fnv_world::cell_of(t.feet[0], t.feet[1]))) }
}

/// A number that differs from one loading screen to the next.
fn roll(n: u32) -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    // SplitMix64's finaliser.
    let mut z = t.wrapping_add(u64::from(n).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// What drawing a screen needs.
#[derive(bevy::ecs::system::SystemParam)]
struct Art<'w> {
    screens: Option<Res<'w, Screens>>,
    data: Option<Res<'w, GameData>>,
    textures: Option<ResMut<'w, TextureCache>>,
    images: Option<ResMut<'w, Assets<Image>>>,
    formats: Option<Res<'w, CompressedImageFormatSupport>>,
    prefer_location: Option<Res<'w, LocationLoadScreens>>,
    look: ResMut<'w, Look>,
}

/// The run starts in the world ([`StartCover`]): bring the screen up in its first frame
/// and hold the game until [`finish`] finds the world ready to play.
#[allow(clippy::too_many_arguments)]
fn begin_start(
    mut commands: Commands,
    asked: Option<Res<StartCover>>,
    mut loading: ResMut<Loading>,
    location: Res<Location>,
    real: Res<Time<Real>>,
    mut clock: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<PlayState>>,
    mut art: Art,
) {
    if asked.is_none() {
        return;
    }
    commands.remove_resource::<StartCover>();
    if loading.active() {
        return;
    }
    // An interior's own screen; outdoors one for anywhere (the start's cell is not known
    // to this frame).
    let to = location.interior.map(|c| Destination { interior: Some(c), exterior: None });
    info!("loading screen: the start is held until the world can be played");
    spawn_screen(&mut commands, &mut loading, &mut art, to.as_ref());
    clock.pause();
    next.set(PlayState::Loading);
    loading.start = Some(StartHold { started: real.elapsed_secs_f64(), ready: None });
    loading.picture_hold = Some(std::time::Instant::now());
}

/// A travel was asked for: bring the screen up and hold the game.
#[allow(clippy::too_many_arguments)]
fn begin(
    mut commands: Commands,
    mut requests: MessageReader<TravelRequest>,
    mut loading: ResMut<Loading>,
    location: Res<Location>,
    real: Res<Time<Real>>,
    mut clock: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<PlayState>>,
    mut art: Art,
) {
    let Some(TravelRequest(travel)) = requests.read().last().copied() else { return };
    if let Some(trip) = loading.trip.as_mut() {
        // Another travel under the screen that is up: it replaces the one waiting, or
        // goes out next frame.
        (trip.travel, trip.sent, trip.frames, trip.ready) = (travel, false, trip.frames.max(SHOW_FRAMES), None);
        return;
    }
    if loading.cover {
        info!("loading screen: travel ignored under a screen held for a restart");
        return;
    }
    if loading.start.is_some() {
        // Under the start's screen: the travel goes out behind it, and the start waits
        // for the destination too.
        loading.trip = Some(Trip { travel, sent: false, frames: SHOW_FRAMES, started: real.elapsed_secs_f64(), ready: None });
        return;
    }
    let to = (!std::mem::take(&mut loading.generic_next)).then(|| destination(&travel, *location));
    loading.plain_once = loading.plain_going_out && location.interior.is_some() && travel.destination(*location).interior.is_none();
    spawn_screen(&mut commands, &mut loading, &mut art, to.as_ref());
    clock.pause();
    next.set(PlayState::Loading);
    loading.trip = Some(Trip { travel, sent: false, frames: 0, started: real.elapsed_secs_f64(), ready: None });
}

/// A [`CoverRequest`]: the screen with no trip; an [`Uncover`]: down again.
#[allow(clippy::too_many_arguments)]
fn hold(
    mut commands: Commands,
    mut up: MessageReader<CoverRequest>,
    mut down: MessageReader<Uncover>,
    mut loading: ResMut<Loading>,
    mut clock: ResMut<Time<Virtual>>,
    (state, mut next): (Res<State<PlayState>>, ResMut<NextState<PlayState>>),
    mut paused: Option<ResMut<crate::pause::Paused>>,
    mut art: Art,
    roots: Query<Entity, With<LoadingRoot>>,
) {
    let asked = up.read().last().copied();
    if down.read().last().is_some() && loading.cover {
        info!("loading screen down (held for a restart)");
        loading.cover = false;
        for e in &roots {
            commands.entity(e).despawn();
        }
        clock.unpause();
        let (back, held) = loading.cover_from.take().unwrap_or((PlayState::Playing, false));
        next.set(back);
        if let Some(p) = paused.as_mut() {
            p.0 = held;
        }
        return;
    }
    let Some(CoverRequest { to }) = asked else { return };
    if loading.active() {
        // A travel's screen is up: it becomes this one, and stays.
        loading.trip = None;
        loading.cover = true;
        return;
    }
    spawn_screen(&mut commands, &mut loading, &mut art, to.as_ref());
    clock.pause();
    loading.cover_from = Some((*state.get(), paused.is_some_and(|p| p.0)));
    next.set(PlayState::Loading);
    loading.cover = true;
}

/// The picture's node: the whole window, whatever the picture's shape [observed: the
/// original's frame at 1280x720 matches the 2:1 picture stretched, loading-screens.md §4].
fn picture_node() -> Node {
    Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }
}

/// The screen's nodes: a picture for `to` (for anywhere without it), its tip and the emblem.
fn spawn_screen(commands: &mut Commands, loading: &mut Loading, art: &mut Art, to: Option<&Destination>) {
    let anywhere = Destination { interior: None, exterior: None };
    let to = to.unwrap_or(&anywhere);
    loading.shown += 1;
    let plain = std::mem::take(&mut loading.plain_once);
    let prefer_location = loading.prefer_location || art.prefer_location.as_deref().is_some_and(|s| s.0);
    let screen = art.screens.as_deref().filter(|_| !plain).and_then(|s| choose_with(&s.0, to, roll(loading.shown), prefer_location));
    if plain {
        info!("loading screen: out of an interior, no picture (tweak)");
    }
    let picture = match (screen.and_then(|s| s.texture.as_deref()), art.data.as_deref(), art.textures.as_mut(), art.images.as_mut()) {
        (Some(path), Some(data), Some(textures), Some(images)) => textures.get(path, true, data, images, art.formats.as_deref()),
        _ => None,
    };
    info!(
        "loading screen up: {} ({}) for {}",
        screen.map_or("none".to_string(), |s| s.edid.clone().unwrap_or_else(|| format!("{:08X}", s.form_id))),
        match (screen.and_then(|s| s.texture.as_deref()), picture.is_some()) {
            (Some(p), true) => p.to_string(),
            (Some(p), false) => format!("{p}: not shown"),
            (None, _) => "no picture".to_string(),
        },
        match (to.interior, to.exterior) {
            (Some(c), _) => format!("interior {c:08X}"),
            (None, Some((w, c))) => format!("worldspace {w:08X} {c:?}"),
            (None, None) => "anywhere".to_string(),
        },
    );
    let root = commands
        .spawn((
            LoadingRoot,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(200),
            // The pointer stops here: nothing beneath is hovered or clicked.
            bevy::ui::FocusPolicy::Block,
        ))
        .id();
    if let Some(image) = picture {
        commands.spawn((ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }, picture_node(), ChildOf(root)));
    }
    // Drawn by `look::fit_tip` (this frame too). No text, no band.
    if let Some(text) = screen.map(|s| s.text.trim()).filter(|t| !t.is_empty() && !loading.hide_tips) {
        let style = screen.and_then(|s| s.tip).unwrap_or(TipStyle::FALLBACK);
        let node = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
        commands.spawn((look::Tip { text: text.to_string(), style, drawn_for: None }, node, ChildOf(root)));
    }
    if let (Some(data), Some(images)) = (art.data.as_deref(), art.images.as_mut()) {
        look::spawn_emblem(commands, root, &mut art.look, data, images, bc_supported(art.formats.as_deref()));
    }
}

/// Once the screen has been drawn: the travel itself, for physics and the renderer.
/// Test aid `NVF_PLAY_TEST_LOADING_SHOT=<png>`: save the window as it is then (the
/// second screen of a run to `<name>-2.png`, and so on).
fn send(mut loading: ResMut<Loading>, mut travel: MessageWriter<Travel>, mut commands: Commands) {
    let shown = loading.shown;
    let Some(trip) = loading.trip.as_mut().filter(|t| !t.sent) else { return };
    trip.frames += 1;
    if trip.frames < SHOW_FRAMES {
        return;
    }
    trip.sent = true;
    travel.write(trip.travel);
    if let Some(path) = std::env::var_os("NVF_PLAY_TEST_LOADING_SHOT").map(std::path::PathBuf::from) {
        let path = match (shown, path.file_stem().and_then(|s| s.to_str())) {
            (2.., Some(stem)) => path.with_file_name(format!("{stem}-{shown}.png")),
            _ => path,
        };
        if let Err(e) = fnv_install::guard_write(&path, None) {
            warn!("test aid: loading screen screenshot not written: {e}");
            return;
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        info!("test aid: loading screen screenshot {}", path.display());
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}

/// What a loading screen still waits for (empty: nothing).
fn missing(loading: &Loading, phys: Option<&PhysState>, scripts: Option<&crate::script::ScriptState>, start: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if loading.render_busy {
        out.push("cells to draw");
    }
    if !phys.is_none_or(|p| p.loading_done()) {
        out.push("cells in physics");
    }
    if loading.actors_busy {
        out.push("actors to draw");
    }
    if start && loading.start_actors.is_some_and(|a| a.under_screen && a.left > 0) {
        out.push(ACTORS_TO_PLACE);
    }
    if start && scripts.is_some_and(|s| s.loading() || s.pending_game_waiting()) {
        out.push("the scripts");
    }
    if loading.pipelines_busy {
        out.push(PIPELINES);
    }
    if loading.pipboy_warming {
        out.push("the Pip-Boy warm-up");
    }
    if loading.lamp_shadows_busy {
        out.push("ALCS");
    }
    out
}

/// What [`missing`] names for pipelines.
const PIPELINES: &str = "pipelines";
/// What [`missing`] names for the start's residents still to draw under its screen.
const ACTORS_TO_PLACE: &str = "actors to place";

/// Take [`ACTORS_TO_PLACE`] off `wait` once the start's screen has waited for it
/// [`PLACE_CAP_SECS`] while they may be drawn (`drawing`; from the first such frame,
/// `since`, afresh once the wait ends or drawing stops), and from then on. Returns whether
/// the cap has just passed.
fn cap_placing(since: &mut Option<f64>, capped: &mut bool, wait: &mut Vec<&'static str>, drawing: bool, now: f64) -> bool {
    if !wait.contains(&ACTORS_TO_PLACE) {
        *since = None;
        return false;
    }
    if !*capped {
        if !drawing {
            *since = None;
            return false;
        }
        if now - *since.get_or_insert(now) < PLACE_CAP_SECS {
            return false;
        }
    }
    wait.retain(|w| *w != ACTORS_TO_PLACE);
    !std::mem::replace(capped, true)
}

/// Take pipelines off `wait` once they alone have held the screen for [`PIPELINE_CAP_SECS`].
/// Returns whether that has just happened.
fn cap_pipelines(since: &mut Option<f64>, capped: &mut bool, wait: &mut Vec<&'static str>, now: f64) -> bool {
    if wait.as_slice() != [PIPELINES] {
        *since = None;
        return false;
    }
    if now - *since.get_or_insert(now) < PIPELINE_CAP_SECS {
        return false;
    }
    wait.clear();
    !std::mem::replace(capped, true)
}

/// [`missing`] with the residents' and the pipelines' caps applied.
fn waiting_for(loading: &mut Loading, phys: Option<&PhysState>, scripts: Option<&crate::script::ScriptState>, start: bool, now: f64) -> Vec<&'static str> {
    let mut wait = missing(loading, phys, scripts, start);
    let drawing = loading.start_actors.is_some_and(|a| a.drawing);
    if cap_placing(&mut loading.placing_since, &mut loading.placing_capped, &mut wait, drawing, now) {
        let left = loading.start_actors.map_or(0, |a| a.left);
        warn!("loading screen: {left} residents in view still not drawn after {PLACE_CAP_SECS:.0} s; going on without them (drawn after the screen)");
    }
    if cap_pipelines(&mut loading.pipelines_since, &mut loading.pipelines_capped, &mut wait, now) {
        warn!("loading screen: pipelines not ready after {PIPELINE_CAP_SECS:.0} s; going on without them");
        loading.pipeline_cap_note = true;
    }
    wait
}

/// What [`LoadPhases`] names for the clouds.
const CLOUDS: &str = "the clouds";

/// Whether a settled screen stays up for the volumetric clouds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudsHold {
    /// Nothing to wait for: off, nothing to draw, failed, or drawn as wanted.
    Free,
    Hold,
    /// Still not drawn as wanted, and the limit has passed: the screen goes without them.
    Limit,
}

/// [`CloudsHold`] for clouds still `busy`, a screen `ready` since then (real time, frames:
/// [`settled`]) and the time `now`. Always ends: the time is the screen's own.
fn clouds_hold(busy: bool, ready: Option<(f64, u32)>, now: f64) -> CloudsHold {
    match ready {
        _ if !busy => CloudsHold::Free,
        Some((since, _)) if now - since < CLOUDS_HOLD_SECS => CloudsHold::Hold,
        _ => CloudsHold::Limit,
    }
}

/// Whether a settled screen stays up this frame ([`clouds_hold`]); says so when the limit ends it.
fn held_for_clouds(busy: bool, ready: Option<(f64, u32)>, now: f64) -> bool {
    let hold = clouds_hold(busy, ready, now);
    if let (CloudsHold::Limit, Some((since, frames))) = (hold, ready) {
        debug!("loading screen: the volumetric clouds not drawn with every texel cast after {:.2} s and {frames} frames; going on without them", now - since);
    }
    hold == CloudsHold::Hold
}

/// Whether `ready` has held for the settle time, counting this frame; `false` resets it.
fn settled(ready: &mut Option<(f64, u32)>, ok: bool, now: f64) -> bool {
    if !ok {
        *ready = None;
        return false;
    }
    let (since, frames) = ready.get_or_insert((now, 0));
    *frames += 1;
    *frames >= SETTLE_FRAMES && now - *since >= SETTLE_SECS
}

/// The line the start's screen logs as it comes down about its residents (`StartActors`):
/// `loading screen: start residents: 16 drawn under the screen, 0 in view left for after it`.
fn start_actors_line(a: &StartActors, capped: bool) -> String {
    let why = match (a.under_screen, capped) {
        (false, _) => " (test aid NVF_START_ACTORS_UNDER_SCREEN=0: all drawn after it)",
        (true, true) => " (the screen stopped waiting for them after its cap)",
        (true, false) => "",
    };
    format!("loading screen: start residents: {} drawn under the screen, {} in view left for after it{why}", a.placed, a.left)
}

/// What a finished loading screen says under the Click to Exit tweak (our words) [design: docs/design/tweaks.md].
const PRESS_LINE: &str = "Press any key to continue";

/// Everything is in: take the screen down and let the game go on. A travel's screen waits
/// for the destination's cells (drawn and in physics) and its actors; the start's
/// ([`StartCover`]) for those, the scripts (made, and a save put back) and its residents
/// in view drawn ([`StartActors`]) too.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn finish(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    phys: Option<Res<PhysState>>,
    mut scripts: Option<ResMut<crate::script::ScriptState>>,
    real: Res<Time<Real>>,
    mut clock: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<PlayState>>,
    roots: Query<Entity, With<LoadingRoot>>,
    (keys, mouse): (Option<Res<ButtonInput<KeyCode>>>, Option<Res<ButtonInput<MouseButton>>>),
) {
    // The player's tweak: the screen waits for a press once all is in [design: docs/design/tweaks.md].
    if loading.waiting_click {
        let pressed = keys.is_some_and(|k| k.get_just_pressed().next().is_some()) || mouse.is_some_and(|m| m.get_just_pressed().next().is_some());
        if !pressed {
            return;
        }
        loading.waiting_click = false;
        info!("loading screen: taken down on a press (tweak)");
        for e in &roots {
            commands.entity(e).despawn();
        }
        clock.unpause();
        next.set(PlayState::Playing);
        return;
    }
    let now = real.elapsed_secs_f64();
    let starting = loading.start.is_some();
    let sent = loading.trip.as_ref().is_some_and(|t| t.sent);
    // What the screen that is up waits for, its caps applied: a travel's once it has gone
    // out, else the start's (none before a travel goes out).
    let wait = if sent {
        Some(waiting_for(&mut loading, phys.as_deref(), None, false, now))
    } else if starting && loading.trip.is_none() {
        Some(waiting_for(&mut loading, phys.as_deref(), scripts.as_deref(), true, now))
    } else {
        None
    };
    // Timed from the screen that is up: the start's for a travel under it.
    if let Some(origin) = loading.start.map(|s| s.started).or(loading.trip.map(|t| t.started)) {
        let mut noted = wait.clone().unwrap_or_else(|| vec!["the travel to go out"]);
        if wait.is_some() && loading.clouds_busy {
            noted.push(CLOUDS);
        }
        loading.phases.note(&noted, now - origin, real.delta_secs_f64());
    }
    let wait = wait.unwrap_or_default();
    let clouds = loading.clouds_busy;
    if sent {
        let trip = loading.trip.as_mut().expect("trip");
        let waited = now - trip.started;
        if !settled(&mut trip.ready, wait.is_empty(), now) {
            if !wait.is_empty() && waited >= TIMEOUT_SECS {
                warn!("loading screen: still waiting for {} after {waited:.0} s; going on without it", wait.join(", "));
            } else {
                return;
            }
        } else if !starting && held_for_clouds(clouds, trip.ready, now) {
            // (Under the start's screen the start waits for them, once.)
            return;
        }
        loading.trip = None;
        if starting {
            // A travel under the start's screen: the start goes on waiting under it.
            return;
        }
        info!("loading screen down after {waited:.2} s");
    } else if starting && loading.trip.is_none() {
        let hold = loading.start.as_mut().expect("start");
        let waited = now - hold.started;
        if !settled(&mut hold.ready, wait.is_empty(), now) {
            if !wait.is_empty() && waited >= TIMEOUT_SECS {
                warn!("loading screen: the start still waits for {} after {waited:.0} s; going on without it", wait.join(", "));
            } else {
                return;
            }
        } else if held_for_clouds(clouds, hold.ready, now) {
            return;
        }
        loading.start = None;
        info!("loading screen down after {waited:.2} s (the start: cells, actors and scripts ready); the test aids and --run-seconds count from now");
        if let Some(a) = loading.start_actors.take() {
            info!("{}", start_actors_line(&a, loading.placing_capped));
        }
        // The player's tweak: a loaded save starts in the pause menu [design: docs/design/tweaks.md]; asked as
        // ShowPauseMenu asks (`crate::pause`).
        if let Some(rt) = scripts.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.restored_from_save && rt.game.tweaks.pause_on_load) {
            rt.game.showoff.b5.pause_asks += 1;
            rt.game.showoff.b5.pause_mode = 0;
            info!("loading screen: the save starts paused (tweak)");
        }
    } else {
        return;
    }
    let phases = std::mem::take(&mut loading.phases);
    info!("loading screen phases: {}", phases.text());
    if loading.clouds_ready {
        info!("loading screen: the volumetric clouds were drawn behind it, every texel cast");
    }
    loading.last_phases = Some(phases);
    (loading.pipelines_since, loading.pipelines_capped) = (None, false);
    (loading.placing_since, loading.placing_capped) = (None, false);
    if loading.click_to_exit {
        loading.waiting_click = true;
        for e in &roots {
            commands.entity(e).with_children(|p| {
                let node = Node { position_type: PositionType::Absolute, bottom: Val::Percent(6.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() };
                p.spawn(node).with_children(|n| {
                    n.spawn((Text::new(PRESS_LINE), TextFont { font_size: FontSize::Px(22.0), ..default() }, TextColor(Color::srgb(1.0, 0.75, 0.3))));
                });
            });
        }
        info!("loading screen: all is in, waiting for a press (tweak)");
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    clock.unpause();
    next.set(PlayState::Playing);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_keep_each_waits_last_frame_and_the_longest_frame() {
        let mut p = LoadPhases::default();
        p.note(&["cells to draw", "pipelines"], 0.5, 0.1);
        p.note(&["cells to draw"], 2.0, 1.4);
        p.note(&["actors to draw"], 3.0, 0.2);
        p.note(&[], 3.3, 0.05);
        assert_eq!(p.until, vec![("cells to draw", 2.0), ("pipelines", 0.5), ("actors to draw", 3.0)]);
        assert_eq!((p.total, p.longest_frame, p.frames), (3.3, 1.4, 4));
        assert!(p.text().starts_with("up 3.30 s; cells to draw 2.00 s, pipelines 0.50 s, actors to draw 3.00 s; longest frame 1.40 s of 4; read "), "{}", p.text());
    }

    #[test]
    fn start_waits_for_pipboy_teardown_even_when_pipelines_are_quiet() {
        let mut loading = Loading { pipboy_warming: true, ..default() };
        assert!(missing(&loading, None, None, true).contains(&"the Pip-Boy warm-up"));
        loading.pipboy_warming = false;
        assert!(missing(&loading, None, None, true).is_empty());
    }

    #[test]
    fn a_screen_waits_for_the_lamp_shadows_pre_draw() {
        let mut loading = Loading { lamp_shadows_busy: true, ..default() };
        assert_eq!(missing(&loading, None, None, false), vec!["ALCS"]);
        loading.lamp_shadows_busy = false;
        assert!(missing(&loading, None, None, false).is_empty());
    }

    /// The volumetric clouds hold a settled screen while not every texel has been cast, and never
    /// past the limit of real time, however many frames that is.
    #[test]
    fn the_clouds_hold_a_settled_screen_up_to_their_limit() {
        use CloudsHold::{Free, Hold, Limit};
        assert_eq!(clouds_hold(false, Some((10.0, 3)), 10.0), Free, "off, nothing to draw, failed or drawn");
        assert_eq!(clouds_hold(false, Some((10.0, 500)), 99.0), Free, "nothing said after the limit either");
        assert_eq!(clouds_hold(true, Some((10.0, 3)), 10.25), Hold);
        assert_eq!(clouds_hold(true, Some((10.0, 9)), 10.0 + CLOUDS_HOLD_SECS - 0.01), Hold, "just before the limit, at nine frames a second");
        assert_eq!(clouds_hold(true, Some((10.0, 4)), 10.0 + CLOUDS_HOLD_SECS), Limit, "the limit");
        assert_eq!(clouds_hold(true, Some((10.0, 10_000)), 10.5), Hold, "no limit of frames");
        assert_eq!(clouds_hold(true, None, 10.0), Limit, "no count to wait by: not waited for");
        assert!(held_for_clouds(true, Some((10.0, 3)), 10.25) && !held_for_clouds(true, Some((10.0, 3)), 10.0 + CLOUDS_HOLD_SECS) && !held_for_clouds(false, Some((10.0, 3)), 10.25));
        // Not one of the waits that start the settle time again.
        assert!(missing(&Loading { clouds_busy: true, ..default() }, None, None, true).is_empty());
    }

    /// Clouds that never come do not hang a travel's or the start's screen, and are named in its
    /// phases.
    #[test]
    fn a_screen_never_hangs_on_the_clouds() {
        for start in [false, true] {
            let mut app = app();
            if start {
                app.insert_resource(StartCover);
            }
            app.update();
            if !start {
                let t = Travel { world: None, interior: None, feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: None };
                app.world_mut().write_message(TravelRequest(t));
            }
            app.world_mut().resource_mut::<Loading>().clouds_busy = true;
            app.update();
            assert!(app.world().resource::<Loading>().active());
            until_down(&mut app);
            assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
            let l = app.world().resource::<Loading>();
            assert!(l.last_phases.as_ref().is_some_and(|p| p.until.iter().any(|(w, _)| *w == CLOUDS)), "start {start}: {:?}", l.last_phases);
        }
    }

    #[test]
    fn the_picture_covers_the_window() {
        let picture = Vec2::new(1024.0, 512.0);
        // A window less wide than the picture: full height, cropped left and right.
        let (size, at) = cover(picture, Vec2::new(1280.0, 800.0));
        assert_eq!((size, at), (Vec2::new(1600.0, 800.0), Vec2::new(-160.0, 0.0)));
        // A wider one: full width, cropped above and below.
        let (size, at) = cover(picture, Vec2::new(2400.0, 1000.0));
        assert_eq!((size, at), (Vec2::new(2400.0, 1200.0), Vec2::new(0.0, -100.0)));
        assert_eq!(cover(picture, Vec2::new(1000.0, 500.0)), (Vec2::new(1000.0, 500.0), Vec2::ZERO));
        assert_eq!(cover(Vec2::ZERO, Vec2::new(8.0, 6.0)), (Vec2::new(8.0, 6.0), Vec2::ZERO));
    }

    #[test]
    fn the_picture_fills_the_window_whatever_its_shape() {
        let node = picture_node();
        assert_eq!((node.position_type, node.width, node.height), (PositionType::Absolute, Val::Percent(100.0), Val::Percent(100.0)));
        assert_eq!((node.left, node.top), (Val::Auto, Val::Auto));
    }

    #[test]
    fn where_a_travel_arrives() {
        let outside = Location { world: 0xDA726, interior: None };
        let indoors = Travel { world: None, interior: Some(0x100), feet: [0.0; 3], heading_deg: 0.0, pitch_deg: None };
        assert_eq!(destination(&indoors, outside), Destination { interior: Some(0x100), exterior: None });
        let out = Travel { world: None, interior: None, feet: [-69_500.0, -5_000.0, 0.0], heading_deg: 0.0, pitch_deg: None };
        let inside = Location { world: 0xDA726, interior: Some(0x100) };
        assert_eq!(destination(&out, inside), Destination { interior: None, exterior: Some((0xDA726, (-17, -2))) });
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_resource(Location { world: 0xDA726, interior: None })
            .init_state::<PlayState>()
            .add_message::<Travel>()
            .configure_sets(Update, (PlaySet::Input, PlaySet::Sync).chain())
            .add_plugins(LoadingPlugin);
        // No plugins to read: no picture, the screen is still shown.
        app
    }

    /// A screen's tip comes up only with text to show (`look::fit_tip` draws the band with
    /// it); the screen and its black are there either way, with no model to read.
    #[test]
    fn a_screen_has_a_tip_only_with_text() {
        for (text, tips) in [("A tip.", 1), ("  ", 0)] {
            let mut app = app();
            let screen = LoadScreen { form_id: 1, edid: None, texture: None, text: text.into(), locations: Vec::new(), main_menu: false, tip: None };
            app.insert_resource(Screens(vec![screen]));
            app.update();
            app.world_mut().write_message(CoverRequest { to: None });
            app.update();
            assert_eq!(app.world_mut().query::<&LoadingRoot>().iter(app.world()).count(), 1);
            assert_eq!(app.world_mut().query::<&look::Tip>().iter(app.world()).count(), tips, "{text:?}");
        }
    }

    fn sent(app: &mut App) -> usize {
        app.world_mut().resource_mut::<Messages<Travel>>().drain().count()
    }

    #[test]
    fn pipelines_hold_the_screen_up_to_their_cap_once_all_else_is_in() {
        let (mut since, mut capped) = (None, false);
        let mut wait = vec![PIPELINES];
        assert!(!cap_pipelines(&mut since, &mut capped, &mut wait, 10.0));
        assert_eq!(wait, [PIPELINES], "held at first");
        assert!(!cap_pipelines(&mut since, &mut capped, &mut wait, 10.0 + PIPELINE_CAP_SECS - 0.1));
        assert_eq!(wait, [PIPELINES], "held just before the cap");
        assert!(cap_pipelines(&mut since, &mut capped, &mut wait, 10.0 + PIPELINE_CAP_SECS), "the cap is noted once");
        assert!(wait.is_empty(), "let go at the cap");
        let mut wait = vec![PIPELINES];
        assert!(!cap_pipelines(&mut since, &mut capped, &mut wait, 30.0), "not noted twice");
        assert!(wait.is_empty(), "still let go");
    }

    #[test]
    fn the_cap_counts_only_while_pipelines_alone_are_waited_for() {
        let (mut since, mut capped) = (None, false);
        assert!(!cap_pipelines(&mut since, &mut capped, &mut vec![PIPELINES], 0.0));
        let mut others = vec!["cells to draw", PIPELINES];
        assert!(!cap_pipelines(&mut since, &mut capped, &mut others, 100.0));
        assert_eq!(others, ["cells to draw", PIPELINES], "other waits keep the whole list");
        assert_eq!(since, None, "and start the count again");
        let mut wait = vec![PIPELINES];
        assert!(!cap_pipelines(&mut since, &mut capped, &mut wait, 101.0));
        assert_eq!(wait, [PIPELINES]);
    }

    #[test]
    fn busy_pipelines_hold_a_travels_screen() {
        let mut app = app();
        app.update();
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: None };
        app.world_mut().write_message(TravelRequest(t));
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(sent(&mut app), 1);
        app.world_mut().resource_mut::<Loading>().pipelines_busy = true;
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs_f64() < SETTLE_SECS * 2.0 {
            app.update();
        }
        assert!(app.world().resource::<Loading>().active(), "held while pipelines compile");
        app.world_mut().resource_mut::<Loading>().pipelines_busy = false;
        let t0 = std::time::Instant::now();
        while app.world().resource::<Loading>().active() {
            assert!(t0.elapsed().as_secs_f64() < 10.0, "the screen never came down");
            app.update();
        }
    }

    #[test]
    fn the_world_holds_until_the_starts_screen_is_drawn_or_the_cap() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let after = |s: f64| t0 + Duration::from_secs_f64(s);
        let mut loading = Loading { picture_hold: Some(t0), ..default() };
        assert!(loading.world_held());
        loading.release_picture_hold(false, after(PICTURE_CAP_SECS - 0.1));
        assert!(loading.world_held(), "not drawn and within the cap");
        loading.release_picture_hold(true, after(0.5));
        assert!(!loading.world_held(), "drawn");
        let mut loading = Loading { picture_hold: Some(t0), ..default() };
        loading.release_picture_hold(false, after(PICTURE_CAP_SECS));
        assert!(!loading.world_held(), "the cap");
        let mut loading = Loading::default();
        loading.release_picture_hold(true, after(9.0));
        assert!(!loading.world_held(), "nothing to let go");
    }

    #[test]
    fn the_screen_is_up_before_the_travel_and_holds_the_game() {
        let mut app = app();
        app.update();
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: None };
        app.world_mut().write_message(TravelRequest(t));
        app.update();
        // Up at once, the clock stopped; the travel has not gone out.
        assert!(app.world().resource::<Loading>().active());
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        assert_eq!(app.world_mut().query::<&LoadingRoot>().iter(app.world()).count(), 1);
        assert_eq!(sent(&mut app), 0);
        app.update();
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Loading);
        assert_eq!(sent(&mut app), 0);
        // The renderer has work from the travel on.
        app.world_mut().resource_mut::<Loading>().render_busy = true;
        app.update();
        assert_eq!(sent(&mut app), 1);
        for _ in 0..10 {
            app.update();
        }
        assert!(app.world().resource::<Loading>().active(), "held while the renderer loads");
        assert_eq!(sent(&mut app), 0);
        // Loaded: down after the settle time, and the game goes on.
        app.world_mut().resource_mut::<Loading>().render_busy = false;
        let t0 = std::time::Instant::now();
        while app.world().resource::<Loading>().active() {
            assert!(t0.elapsed().as_secs_f64() < 10.0, "the screen never came down");
            app.update();
        }
        assert!(t0.elapsed().as_secs_f64() >= SETTLE_SECS * 0.9);
        app.update();
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        assert_eq!(app.world_mut().query::<&LoadingRoot>().iter(app.world()).count(), 0);
    }

    /// EnableLoadingMenu before a move (`crate::script`): that travel's screen, and only
    /// that one, is one for anywhere.
    #[test]
    fn enable_loading_menu_screen_is_for_anywhere() {
        let mut app = app();
        app.update();
        app.world_mut().resource_mut::<Loading>().generic_next = true;
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: None };
        app.world_mut().write_message(TravelRequest(t));
        app.update();
        let l = app.world().resource::<Loading>();
        assert!(l.active() && !l.generic_next, "taken by the travel's screen");
    }

    #[test]
    fn a_held_screen_coming_down_goes_back_to_the_menu_it_came_from() {
        let mut app = app();
        app.insert_resource(crate::pause::Paused(true));
        // As the pause menu's own `close` on leaving the menu state.
        app.add_systems(OnExit(PlayState::Menu), |mut p: ResMut<crate::pause::Paused>| p.0 = false);
        app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Menu);
        app.update();
        app.world_mut().write_message(CoverRequest { to: None });
        app.update();
        app.update();
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Loading);
        assert!(!app.world().resource::<crate::pause::Paused>().0);
        // The new run came to nothing: the menu again, still holding the game.
        app.world_mut().write_message(Uncover);
        app.update();
        app.update();
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Menu);
        assert!(app.world().resource::<crate::pause::Paused>().0);
        assert!(!app.world().resource::<Loading>().active());
    }

    /// A menu left open under a screen (the main menu while a save loads) gets no keys or
    /// clicks; they count again once it is down.
    #[test]
    fn no_input_gets_through_a_screen() {
        let mut app = app();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>();
        let press = |app: &mut App| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        };
        app.update();
        app.world_mut().write_message(CoverRequest { to: None });
        app.update();
        press(&mut app);
        app.update();
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ArrowDown));
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().just_pressed(MouseButton::Left));
        app.world_mut().write_message(Uncover);
        app.update();
        app.update();
        assert!(!app.world().resource::<Loading>().active());
        press(&mut app);
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::ArrowDown));
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }

    /// Come down once nothing is missing, after the settle time (at most 10 s of frames).
    fn until_down(app: &mut App) {
        let t0 = std::time::Instant::now();
        while app.world().resource::<Loading>().active() {
            assert!(t0.elapsed().as_secs_f64() < 10.0, "the screen never came down");
            app.update();
        }
        app.update();
    }

    #[test]
    fn the_start_is_held_until_the_actors_are_drawn() {
        let mut app = app();
        app.insert_resource(StartCover);
        app.update();
        // Up in the first frame, the game held; the aids' clock does not run under it.
        assert!(app.world().resource::<Loading>().starting());
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        assert_eq!(app.world_mut().query::<&LoadingRoot>().iter(app.world()).count(), 1);
        app.world_mut().resource_mut::<Loading>().actors_busy = true;
        for _ in 0..10 {
            app.update();
        }
        assert!(app.world().resource::<Loading>().starting(), "held while actors are still read");
        app.world_mut().resource_mut::<Loading>().actors_busy = false;
        until_down(&mut app);
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        assert_eq!(app.world_mut().query::<&LoadingRoot>().iter(app.world()).count(), 0);
        assert!(!app.world().contains_resource::<StartCover>());
    }

    #[test]
    fn the_aids_clock_stands_still_under_the_start_screen() {
        let mut app = app();
        app.init_resource::<crate::pause::PlayClock>().add_systems(Update, crate::pause::tick_play_clock.before(PlaySet::Input));
        app.insert_resource(StartCover);
        app.update();
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs_f64() < 0.1 {
            app.update();
        }
        assert!(app.world().resource::<Loading>().starting());
        assert_eq!(app.world().resource::<crate::pause::PlayClock>().stopped_secs(), 0.0);
    }

    /// The residents nvforever draws under the start's screen: it waits while any in view
    /// is still to draw, names the wait in its phases, and logs at the end how many were
    /// drawn under it.
    #[test]
    fn the_start_is_held_until_its_residents_in_view_are_placed() {
        let mut app = app();
        app.insert_resource(StartCover);
        app.update();
        app.world_mut().resource_mut::<Loading>().start_actors = Some(StartActors { placed: 0, left: 5, under_screen: true, drawing: false });
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs_f64() < SETTLE_SECS * 2.0 {
            app.update();
        }
        let l = app.world().resource::<Loading>();
        assert!(l.starting(), "held while residents in view are still to draw, before they may be");
        assert_eq!(l.placing_since, None, "the cap counts only once they may be drawn");
        app.world_mut().resource_mut::<Loading>().start_actors = Some(StartActors { placed: 3, left: 2, under_screen: true, drawing: true });
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs_f64() < SETTLE_SECS * 2.0 {
            app.update();
        }
        let l = app.world().resource::<Loading>();
        assert!(l.starting(), "held while residents in view are still to draw");
        assert!(l.placing_since.is_some(), "the cap counts while they are drawn");
        assert!(missing(l, None, None, true).contains(&ACTORS_TO_PLACE));
        assert!(l.phases.until.iter().any(|(w, _)| *w == ACTORS_TO_PLACE), "named in the phases: {:?}", l.phases.until);
        assert!(!missing(l, None, None, false).contains(&ACTORS_TO_PLACE), "not a travel's wait");
        app.world_mut().resource_mut::<Loading>().start_actors = Some(StartActors { placed: 5, left: 0, under_screen: true, drawing: true });
        until_down(&mut app);
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
        let l = app.world().resource::<Loading>();
        assert_eq!(l.start_actors, None, "taken at the end");
        assert!(l.last_phases.as_ref().is_some_and(|p| p.until.iter().any(|(w, _)| *w == ACTORS_TO_PLACE)));
        assert!(!l.placing_capped());
    }

    /// Test aid `NVF_START_ACTORS_UNDER_SCREEN=0`: the residents are counted but not waited
    /// for, so the start comes down as it did before they were drawn under it.
    #[test]
    fn with_the_aid_off_the_start_does_not_wait_for_its_residents() {
        let mut app = app();
        app.insert_resource(StartCover);
        app.update();
        app.world_mut().resource_mut::<Loading>().start_actors = Some(StartActors { placed: 0, left: 16, under_screen: false, drawing: false });
        assert!(missing(app.world().resource::<Loading>(), None, None, true).is_empty());
        until_down(&mut app);
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
        assert_eq!(start_actors_line(&StartActors { placed: 0, left: 16, under_screen: false, drawing: false }, false), "loading screen: start residents: 0 drawn under the screen, 16 in view left for after it (test aid NVF_START_ACTORS_UNDER_SCREEN=0: all drawn after it)");
        assert_eq!(start_actors_line(&StartActors { placed: 16, left: 0, under_screen: true, drawing: true }, false), "loading screen: start residents: 16 drawn under the screen, 0 in view left for after it");
    }

    #[test]
    fn the_residents_hold_the_start_up_to_their_cap() {
        let (mut since, mut capped) = (None, false);
        // Waited for before they may be drawn (the cells in physics, the scripts): no count
        // starts, however long that takes (#3081 review: a slow cold start).
        let mut wait = vec!["cells in physics", ACTORS_TO_PLACE];
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, false, 1.0));
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, false, 1.0 + PLACE_CAP_SECS * 3.0));
        assert_eq!((since, capped), (None, false));
        assert_eq!(wait, ["cells in physics", ACTORS_TO_PLACE], "still held");
        let mut wait = vec!["cells to draw", ACTORS_TO_PLACE, PIPELINES];
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, true, 10.0));
        assert_eq!(wait, ["cells to draw", ACTORS_TO_PLACE, PIPELINES], "held at first, the other waits kept");
        assert_eq!(since, Some(10.0), "counted from the first frame they may be drawn");
        // Counted from there, whatever else it waits for.
        let mut wait = vec![ACTORS_TO_PLACE];
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, true, 10.0 + PLACE_CAP_SECS - 0.1));
        assert_eq!(wait, [ACTORS_TO_PLACE], "held just before the cap");
        let mut wait = vec![ACTORS_TO_PLACE, PIPELINES];
        assert!(cap_placing(&mut since, &mut capped, &mut wait, true, 10.0 + PLACE_CAP_SECS), "the cap is noted once");
        assert_eq!(wait, [PIPELINES], "let go at the cap; the pipelines keep their own");
        // nvforever stops drawing them under the screen once it has passed.
        let mut wait = vec![ACTORS_TO_PLACE];
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, false, 40.0), "not noted twice");
        assert!(wait.is_empty(), "still let go");
        // Nothing to place: no count starts.
        let (mut since, mut capped) = (None, false);
        assert!(!cap_placing(&mut since, &mut capped, &mut vec![PIPELINES], true, 5.0));
        assert_eq!((since, capped), (None, false));
    }

    /// The count ends with the wait for them: a second wait on the same screen (residents
    /// in view again after a travel under it) gets the whole cap, and so does one that
    /// could not be drawn for a while.
    #[test]
    fn the_residents_cap_counts_afresh_once_the_wait_ends() {
        let (mut since, mut capped) = (None, false);
        assert!(!cap_placing(&mut since, &mut capped, &mut vec![ACTORS_TO_PLACE], true, 10.0));
        assert!(!cap_placing(&mut since, &mut capped, &mut vec!["cells to draw"], true, 12.0));
        assert_eq!(since, None, "the wait ended");
        let mut wait = vec![ACTORS_TO_PLACE];
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, true, 12.5));
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, true, 10.0 + PLACE_CAP_SECS + 0.1));
        assert_eq!(wait, [ACTORS_TO_PLACE], "held: counted from 12.5, not 10");
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, false, 14.0));
        assert_eq!(since, None, "not drawn for a while: counted afresh");
        assert!(!cap_placing(&mut since, &mut capped, &mut wait, true, 15.0));
        assert!(cap_placing(&mut since, &mut capped, &mut wait, true, 15.0 + PLACE_CAP_SECS));
        assert!(wait.is_empty());
    }

    /// The cap in a start: residents that are never drawn (their parts never read) do not
    /// hold the screen past it; it comes down, says so, and nvforever stops drawing them
    /// under it.
    #[test]
    fn a_start_whose_residents_are_never_placed_comes_down_at_the_cap() {
        let mut app = app();
        app.insert_resource(StartCover);
        app.update();
        app.world_mut().resource_mut::<Loading>().start_actors = Some(StartActors { placed: 1, left: 4, under_screen: true, drawing: true });
        for _ in 0..5 {
            app.update();
        }
        let since = app.world().resource::<Loading>().placing_since;
        assert!(since.is_some(), "the count started");
        assert!(app.world().resource::<Loading>().starting());
        // As if it had waited for them the whole cap already.
        app.world_mut().resource_mut::<Loading>().placing_since = since.map(|s| s - PLACE_CAP_SECS);
        app.update();
        assert!(app.world().resource::<Loading>().placing_capped(), "capped");
        until_down(&mut app);
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
        let l = app.world().resource::<Loading>();
        assert!(!l.placing_capped() && l.placing_since.is_none(), "the next screen counts afresh");
        // The phases end the wait for them at the cap, not when the screen came down (it
        // settled after the cap).
        let p = l.last_phases.as_ref().expect("phases");
        let placing = p.until.iter().find(|(w, _)| *w == ACTORS_TO_PLACE).map(|(_, t)| *t).expect("named in the phases");
        assert!(p.total - placing >= SETTLE_SECS, "{p:?}");
        assert_eq!(start_actors_line(&StartActors { placed: 1, left: 4, under_screen: true, drawing: false }, true), "loading screen: start residents: 1 drawn under the screen, 4 in view left for after it (the screen stopped waiting for them after its cap)");
    }

    #[test]
    fn a_travel_screen_waits_for_the_destinations_actors() {
        let mut app = app();
        app.update();
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: None };
        app.world_mut().write_message(TravelRequest(t));
        app.world_mut().resource_mut::<Loading>().actors_busy = true;
        for _ in 0..10 {
            app.update();
        }
        assert!(app.world().resource::<Loading>().travelling(), "held while actors are still read");
        app.world_mut().resource_mut::<Loading>().actors_busy = false;
        until_down(&mut app);
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
    }

    #[test]
    fn a_travel_under_the_start_screen_goes_out_and_the_start_waits_on() {
        let mut app = app();
        app.insert_resource(StartCover);
        app.update();
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0, pitch_deg: None };
        app.world_mut().write_message(TravelRequest(t));
        app.world_mut().resource_mut::<Loading>().actors_busy = true;
        app.update();
        app.update();
        assert_eq!(sent(&mut app), 1, "the travel goes out behind the start's screen");
        assert_eq!(app.world_mut().query::<&LoadingRoot>().iter(app.world()).count(), 1, "one screen");
        for _ in 0..10 {
            app.update();
        }
        assert!(app.world().resource::<Loading>().starting());
        app.world_mut().resource_mut::<Loading>().actors_busy = false;
        until_down(&mut app);
        assert!(!app.world().resource::<Loading>().active());
        assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
    }

    #[test]
    fn location_load_screens_setting_defaults_off_syncs_and_round_trips_ini() {
        use bevy::app::PostStartup;
        use bevy::ecs::system::RunSystemOnce;
        use bevy::MinimalPlugins;

        let dir = std::env::temp_dir().join(format!("nvf-location-load-screens-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");

        let make_app = |ini_path: &std::path::Path| {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
                .init_state::<PlayState>()
                .insert_resource(crate::settings::SettingsFile(Some(ini_path.to_path_buf())))
                .init_resource::<crate::settings::GivenSwitches>()
                .add_plugins(LoadingPlugin)
                .add_systems(PostStartup, crate::settings::apply_stored);
            app.world_mut().run_schedule(PostStartup);
            app
        };

        // 1. Fresh start with no INI: defaults to off, within note budget.
        let mut app = make_app(&path);
        assert!(!app.world().resource::<LocationLoadScreens>().0, "must default to off");
        assert!(!app.world().resource::<Loading>().prefer_location, "Loading::prefer_location defaults off");
        let idx = app.world().resource::<crate::settings::Settings>().index("location-load-screens").expect("setting registered");
        let row = &app.world().resource::<crate::settings::Settings>().list()[idx];
        assert_eq!(crate::settings::note_over_budget(row.note), None, "note must fit within 200-char budget");

        // Live sync propagates to Loading::prefer_location and persists to NVForever.ini.
        assert_eq!(crate::settings::flip(app.world_mut(), idx), Some(true));
        app.world_mut().run_system_once(sync_location_load_screens).unwrap();
        assert!(app.world().resource::<LocationLoadScreens>().0);
        assert!(app.world().resource::<Loading>().prefer_location);
        let ini = std::fs::read_to_string(&path).unwrap();
        assert!(ini.contains("location-load-screens=1"), "persisted to NVForever.ini: {ini}");

        // 2. Next start loads ON from NVForever.ini.
        let mut reloaded = make_app(&path);
        assert!(reloaded.world().resource::<LocationLoadScreens>().0, "restored from NVForever.ini");

        // 3. Flipping back off writes 0 and restores off on next start.
        let idx2 = reloaded.world().resource::<crate::settings::Settings>().index("location-load-screens").unwrap();
        assert_eq!(crate::settings::flip(reloaded.world_mut(), idx2), Some(false));
        let ini_off = std::fs::read_to_string(&path).unwrap();
        assert!(ini_off.contains("location-load-screens=0"), "off persisted: {ini_off}");
        let reloaded_off = make_app(&path);
        assert!(!reloaded_off.world().resource::<LocationLoadScreens>().0, "off restored from NVForever.ini");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn location_load_screens_consumer_prefers_matching_screen_when_enabled_and_preserves_vanilla_when_off() {
        use fnv_world::loadscreen::ScreenLocation;

        const CELL: u32 = 0x106FE5;
        let screens = vec![
            LoadScreen { form_id: 1, edid: Some("GenericA".into()), texture: None, text: "Generic tip".into(), locations: vec![], main_menu: false, tip: None },
            LoadScreen {
                form_id: 2,
                edid: Some("SaloonOwn".into()),
                texture: None,
                text: "Saloon tip".into(),
                locations: vec![ScreenLocation::Direct(CELL)],
                main_menu: false,
                tip: None,
            },
        ];

        let run_trips = |prefer: bool| -> Vec<String> {
            let mut app = app();
            app.insert_resource(Screens(screens.clone()));
            app.world_mut().resource_mut::<LocationLoadScreens>().0 = prefer;
            app.update();
            let mut tips = Vec::new();
            for _ in 0..8 {
                let t = Travel { world: None, interior: Some(CELL), feet: [1.0, 2.0, 3.0], heading_deg: 0.0, pitch_deg: None };
                app.world_mut().write_message(TravelRequest(t));
                app.update();
                let tip = app.world_mut().query::<&look::Tip>().iter(app.world()).next().expect("tip spawned").text.clone();
                tips.push(tip);
                until_down(&mut app);
            }
            tips
        };

        // Default off: mixes location-specific and generic tips across rolls (vanilla behavior).
        let vanilla = run_trips(false);
        assert!(vanilla.iter().any(|t| t == "Generic tip"), "off must still pick generic screens: {vanilla:?}");
        assert!(vanilla.iter().any(|t| t == "Saloon tip"), "off must also pick location screens: {vanilla:?}");

        // Enabled: every trip to CELL chooses the matching location-specific screen.
        let preferred = run_trips(true);
        assert!(preferred.iter().all(|t| t == "Saloon tip"), "on must prefer matching location screens: {preferred:?}");
    }
}
