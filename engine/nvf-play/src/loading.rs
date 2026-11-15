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
//!   the graphics card behind the picture.
//! - A run that starts in the world ([`StartCover`]) is held under the screen from its
//!   first frame until the same is in and the scripts are made (a save put back into
//!   them); the test aids' clock and `--run-seconds` count from when it comes down
//!   (docs/design/loading-screen.md §1a).
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
use fnv_world::loadscreen::{choose, load_screens, Destination, LoadScreen, TipStyle};
use nvf_assets::{GameData, TextureCache};

/// Frames the screen is up before the travel is applied [engineering choice: the frame
/// it is spawned in and one more, so the picture is on screen before the long frames].
pub const SHOW_FRAMES: u32 = 2;

/// Real seconds (and at least [`SETTLE_FRAMES`] frames) the screen stays after the last
/// cell is in [engineering choice].
pub const SETTLE_SECS: f64 = 0.25;
pub const SETTLE_FRAMES: u32 = 3;

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

/// Seamless doors' fade (`crate::seamless`, step 2): real seconds the view takes to go
/// black before the travel goes out, and to come back once the destination is in
/// [engineering choice: short enough to read as a cut through the door, long enough not
/// to flash].
pub const FADE_OUT_SECS: f64 = 0.2;
pub const FADE_IN_SECS: f64 = 0.3;
/// Real seconds after the travel went out at which a fade still waiting shows the
/// loading picture after all [engineering choice: a place not prepared ahead].
pub const FADE_PICTURE_AFTER_SECS: f64 = 0.75;
/// The settle time behind a fade [engineering choice: what was prepared ahead is on the
/// graphics card already; the fade in covers the rest].
pub const FADE_SETTLE_SECS: f64 = 0.05;

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
    /// A fade through black rather than the picture (seamless doors), and whether the
    /// picture came up after all ([`FADE_PICTURE_AFTER_SECS`]).
    fade: Option<FadeTrip>,
}

#[derive(Debug, Clone, Copy)]
struct FadeTrip {
    /// Real time the travel went out.
    sent_at: Option<f64>,
    picture: bool,
    /// The trip's destination, for the picture if it comes.
    to: Option<Destination>,
    /// What it waited for last, as [`waits_key`] (logged when it changes).
    waits: Option<u64>,
}

/// The black veil of a fade (seamless doors). Not a [`LoadingRoot`]: it stays while the
/// view fades back in, after the trip is over.
#[derive(Component)]
struct Veil;

/// The veil's state: fading out since (real time) with the trip, or fading back in since.
#[derive(Resource, Default)]
struct VeilState {
    out_since: Option<f64>,
    in_since: Option<f64>,
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
    trip: Option<Trip>,
    /// The start of the run, held until the world can be played ([`StartCover`]).
    start: Option<StartHold>,
    /// Actors around the player whose parts are still being read, so they are not drawn
    /// yet. Set by nvforever every frame (`readahead::actors_waiting`); stays false
    /// without it.
    pub actors_busy: bool,
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
    /// Since when only pipelines have been waited for (real time), and whether
    /// [`PIPELINE_CAP_SECS`] has passed this screen.
    pipelines_since: Option<f64>,
    pipelines_capped: bool,
    /// The cap has just passed: nvforever logs which pipelines are late.
    pub pipeline_cap_note: bool,
    /// The start's screen is not on the window yet (since this real time): the world holds
    /// off, so that its reads and pipelines do not come first ([`Loading::world_held`]).
    picture_hold: Option<std::time::Instant>,
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
}

/// The loading screens of the plugins.
#[derive(Resource)]
struct Screens(Vec<LoadScreen>);

#[derive(Component)]
struct LoadingRoot;

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Loading>()
            .init_resource::<Look>()
            .add_message::<TravelRequest>()
            .add_message::<crate::seamless::OpenPortal>()
            .init_resource::<crate::seamless::Activated>()
            .add_message::<CoverRequest>()
            .add_message::<Uncover>()
            .add_systems(Startup, read_screens)
            .add_systems(
                PreUpdate,
                hold_input.after(bevy::input::InputSystems).after(crate::pad::menu_keys).after(crate::test_aid::real_presses).before(bevy::ui::UiSystems::Focus),
            )
            .init_resource::<VeilState>()
            .init_resource::<crate::seamless::TravelKeeps>()
            .add_systems(Update, (send.in_set(PlaySet::Input), (begin_start, begin, hold, look::fit_tip, look::turn_emblem).chain().in_set(PlaySet::Sync)))
            .add_systems(Last, (finish, veil).chain());
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

/// What [`begin`] needs to open a door onto its other side (seamless doors).
type SeamlessDoors<'w> = (Option<Res<'w, crate::seamless::LoadDoors>>, Option<Res<'w, crate::seamless::PortalDoors>>, MessageWriter<'w, crate::seamless::OpenPortal>, ResMut<'w, crate::seamless::Activated>);

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
    (seamless, mut veil): (Option<Res<crate::seamless::Seamless>>, ResMut<VeilState>),
    mut seamless_doors: SeamlessDoors,
) {
    let Some(TravelRequest(travel)) = requests.read().last().copied() else { return };
    // Seamless doors (step 4): a door's script that let the player through sends the
    // door's own travel; the door opens onto its other side instead, already activated.
    if let (Some(doors), Some(portals), false) = (seamless_doors.0.as_deref(), seamless_doors.1.as_deref(), loading.active()) {
        if let Some(door) = crate::seamless::door_of_travel(doors, portals, &travel) {
            info!("loading screen: none: door {door:08X} opens onto its other side (seamless doors; its activation ran)");
            seamless_doors.2.write(crate::seamless::OpenPortal { door });
            seamless_doors.3 .0.insert(door);
            loading.generic_next = false;
            loading.arriving = false;
            return;
        }
    }
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
        loading.trip = Some(Trip { travel, sent: false, frames: SHOW_FRAMES, started: real.elapsed_secs_f64(), ready: None, fade: None });
        return;
    }
    let to = (!std::mem::take(&mut loading.generic_next)).then(|| destination(&travel, *location));
    let now = real.elapsed_secs_f64();
    // Seamless doors: a fade through black, for a door's trip and a script's move alike;
    // the picture only if it takes long (`FADE_PICTURE_AFTER_SECS`).
    let fade = seamless.is_some_and(|s| s.fade).then_some(FadeTrip { sent_at: None, picture: false, to, waits: None });
    if fade.is_some() {
        info!("loading screen: a fade through black (seamless doors) for {}", describe(to.as_ref()));
        commands.spawn((
            Veil,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(Color::BLACK.with_alpha(0.0)),
            GlobalZIndex(199),
            bevy::ui::FocusPolicy::Block,
        ));
        *veil = VeilState { out_since: Some(now), in_since: None };
        loading.shown += 1;
    } else {
        spawn_screen(&mut commands, &mut loading, &mut art, to.as_ref());
    }
    clock.pause();
    next.set(PlayState::Loading);
    loading.trip = Some(Trip { travel, sent: false, frames: 0, started: now, ready: None, fade });
}

/// A destination for the log.
fn describe(to: Option<&Destination>) -> String {
    match to.map(|t| (t.interior, t.exterior)) {
        Some((Some(c), _)) => format!("interior {c:08X}"),
        Some((None, Some((w, c)))) => format!("worldspace {w:08X} {c:?}"),
        _ => "anywhere".to_string(),
    }
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
    let screen = art.screens.as_deref().and_then(|s| choose(&s.0, to, roll(loading.shown)));
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
    if let Some(text) = screen.map(|s| s.text.trim()).filter(|t| !t.is_empty()) {
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
#[allow(clippy::type_complexity)]
fn send(
    mut loading: ResMut<Loading>,
    mut travel: MessageWriter<Travel>,
    mut commands: Commands,
    real: Res<Time<Real>>,
    (seamless, d, location, player, phys, mut keeps): (
        Option<Res<crate::seamless::Seamless>>,
        Option<Res<PlayWorld>>,
        Res<Location>,
        Option<Res<crate::player::Player>>,
        Option<Res<PhysState>>,
        ResMut<crate::seamless::TravelKeeps>,
    ),
) {
    let shown = loading.shown;
    let now = real.elapsed_secs_f64();
    let Some(trip) = loading.trip.as_mut().filter(|t| !t.sent) else { return };
    trip.frames += 1;
    if trip.frames < SHOW_FRAMES {
        return;
    }
    if let Some(f) = trip.fade.as_mut() {
        // The view goes black first.
        if now - trip.started < FADE_OUT_SECS {
            return;
        }
        f.sent_at = Some(now);
    }
    trip.sent = true;
    // Seamless doors (step 3): whether the place left stays loaded, decided once here
    // for physics and the renderer (`seamless::TravelKeeps`).
    let feet = phys.as_deref().and_then(|p| p.player.as_ref()).map(|c| c.feet).or(player.as_deref().map(|p| p.feet));
    let keep = match (seamless.as_deref().filter(|s| s.keep), d.as_deref(), feet) {
        (Some(_), Some(d), Some(feet)) => crate::seamless::keeps(d, *location, feet, &trip.travel),
        _ => false,
    };
    keeps.0 = Some((trip.travel, keep));
    if seamless.is_some_and(|s| s.keep) {
        info!("seamless doors: the place left {}", if keep { "stays loaded" } else { "is dropped (another worldspace, or the two places overlap)" });
    }
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
    if start && scripts.is_some_and(|s| s.loading() || s.pending_game_waiting()) {
        out.push("the scripts");
    }
    if loading.pipelines_busy {
        out.push(PIPELINES);
    }
    if loading.pipboy_warming {
        out.push("the Pip-Boy warm-up");
    }
    out
}

/// What [`missing`] names for pipelines.
const PIPELINES: &str = "pipelines";

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

/// [`missing`] with the pipelines' cap applied.
fn waiting_for(loading: &mut Loading, phys: Option<&PhysState>, scripts: Option<&crate::script::ScriptState>, start: bool, now: f64) -> Vec<&'static str> {
    let mut wait = missing(loading, phys, scripts, start);
    if cap_pipelines(&mut loading.pipelines_since, &mut loading.pipelines_capped, &mut wait, now) {
        warn!("loading screen: pipelines not ready after {PIPELINE_CAP_SECS:.0} s; going on without them");
        loading.pipeline_cap_note = true;
    }
    wait
}

/// What a screen waits for as a number, to see it change (FNV-1a over the names).
fn waits_key(wait: &[&str]) -> u64 {
    wait.iter().flat_map(|w| w.bytes().chain([0])).fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// [`settled`] behind a fade: [`FADE_SETTLE_SECS`] and two frames.
fn fade_settled(ready: &mut Option<(f64, u32)>, ok: bool, now: f64) -> bool {
    if !ok {
        *ready = None;
        return false;
    }
    let (since, frames) = ready.get_or_insert((now, 0));
    *frames += 1;
    *frames >= 2 && now - *since >= FADE_SETTLE_SECS
}

/// The fade's veil: black over [`FADE_OUT_SECS`] as a trip begins, back over
/// [`FADE_IN_SECS`] once it is over, then gone.
fn veil(mut commands: Commands, real: Res<Time<Real>>, mut state: ResMut<VeilState>, mut veils: Query<(Entity, &mut BackgroundColor), With<Veil>>) {
    if veils.is_empty() {
        return;
    }
    let now = real.elapsed_secs_f64();
    let alpha = match (state.out_since, state.in_since) {
        (_, Some(t)) => 1.0 - ((now - t) / FADE_IN_SECS).clamp(0.0, 1.0),
        (Some(t), None) => ((now - t) / FADE_OUT_SECS).clamp(0.0, 1.0),
        (None, None) => 0.0,
    };
    let gone = alpha <= 0.0 && state.in_since.is_some();
    for (e, mut c) in &mut veils {
        if gone {
            commands.entity(e).despawn();
        } else {
            c.0 = Color::BLACK.with_alpha(alpha as f32);
        }
    }
    if gone {
        *state = VeilState::default();
    }
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

/// Everything is in: take the screen down and let the game go on. A travel's screen waits
/// for the destination's cells (drawn and in physics) and its actors; the start's
/// ([`StartCover`]) for those and the scripts too (made, and a save put back).
#[allow(clippy::too_many_arguments)]
fn finish(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    phys: Option<Res<PhysState>>,
    scripts: Option<Res<crate::script::ScriptState>>,
    real: Res<Time<Real>>,
    mut clock: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<PlayState>>,
    roots: Query<Entity, With<LoadingRoot>>,
    (mut art, mut veil): (Art, ResMut<VeilState>),
) {
    let now = real.elapsed_secs_f64();
    let starting = loading.start.is_some();
    if loading.trip.as_ref().is_some_and(|t| t.sent) {
        let wait = waiting_for(&mut loading, phys.as_deref(), None, false, now);
        // A fade still waiting long after the travel: the picture after all.
        let late = loading.trip.as_ref().and_then(|t| t.fade).filter(|f| !f.picture && !wait.is_empty() && f.sent_at.is_some_and(|at| now - at >= FADE_PICTURE_AFTER_SECS));
        if let Some(f) = late {
            info!("loading screen: the fade still waits for {} after {FADE_PICTURE_AFTER_SECS:.2} s: the picture comes up", wait.join(", "));
            spawn_screen(&mut commands, &mut loading, &mut art, f.to.as_ref());
            if let Some(f) = loading.trip.as_mut().and_then(|t| t.fade.as_mut()) {
                f.picture = true;
            }
        }
        let trip = loading.trip.as_mut().expect("trip");
        let waited = now - trip.started;
        if let Some(f) = trip.fade.as_mut().filter(|f| f.waits != Some(waits_key(&wait))) {
            info!("loading screen: the fade waits for {} ({waited:.2} s)", if wait.is_empty() { "nothing".to_string() } else { wait.join(", ") });
            f.waits = Some(waits_key(&wait));
        }
        let faded = trip.fade.is_some();
        let ok = wait.is_empty();
        let done = if faded { fade_settled(&mut trip.ready, ok, now) } else { settled(&mut trip.ready, ok, now) };
        if !done {
            if !wait.is_empty() && waited >= TIMEOUT_SECS {
                warn!("loading screen: still waiting for {} after {waited:.0} s; going on without it", wait.join(", "));
            } else {
                return;
            }
        }
        loading.trip = None;
        if starting {
            // A travel under the start's screen: the start goes on waiting under it.
            return;
        }
        if faded {
            info!("loading screen down after {waited:.2} s (a fade through black: the view comes back over {FADE_IN_SECS:.2} s)");
            *veil = VeilState { out_since: None, in_since: Some(now) };
        } else {
            info!("loading screen down after {waited:.2} s");
        }
    } else if starting && loading.trip.is_none() {
        let wait = waiting_for(&mut loading, phys.as_deref(), scripts.as_deref(), true, now);
        let hold = loading.start.as_mut().expect("start");
        let waited = now - hold.started;
        if !settled(&mut hold.ready, wait.is_empty(), now) {
            if !wait.is_empty() && waited >= TIMEOUT_SECS {
                warn!("loading screen: the start still waits for {} after {waited:.0} s; going on without it", wait.join(", "));
            } else {
                return;
            }
        }
        loading.start = None;
        info!("loading screen down after {waited:.2} s (the start: cells, actors and scripts ready); the test aids and --run-seconds count from now");
    } else {
        return;
    }
    (loading.pipelines_since, loading.pipelines_capped) = (None, false);
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
    fn start_waits_for_pipboy_teardown_even_when_pipelines_are_quiet() {
        let mut loading = Loading { pipboy_warming: true, ..default() };
        assert!(missing(&loading, None, None, true).contains(&"the Pip-Boy warm-up"));
        loading.pipboy_warming = false;
        assert!(missing(&loading, None, None, true).is_empty());
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
        let indoors = Travel { world: None, interior: Some(0x100), feet: [0.0; 3], heading_deg: 0.0 };
        assert_eq!(destination(&indoors, outside), Destination { interior: Some(0x100), exterior: None });
        let out = Travel { world: None, interior: None, feet: [-69_500.0, -5_000.0, 0.0], heading_deg: 0.0 };
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
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0 };
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
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0 };
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
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0 };
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

    #[test]
    fn a_travel_screen_waits_for_the_destinations_actors() {
        let mut app = app();
        app.update();
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0 };
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
        let t = Travel { world: None, interior: Some(0x100), feet: [1.0, 2.0, 3.0], heading_deg: 90.0 };
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
}
