//! The main menu (nvforever `--menu`): what the game opens on. Continue loads the newest
//! save, New Game starts the opening in Doc Mitchell's house, Load Game shows the saves,
//! Settings the switches, Quit ends the program. Continue and Load Game are greyed while
//! there is no save.
//!
//! From the original [observed: its main menu file `menus\options\start_menu.xml` and the
//! owner's screenshot of its main menu, both in C:\fnv-private]: the entries in a column
//! at the right, right-aligned, the column centred top to bottom; font 7; the interface's
//! amber; the logo (`textures\interface\main\main_title.dds`, read from the owner's
//! archives at run time) at the left. Its entries Credits and Downloadable Content are
//! left out [owner, 2026-10-15].
//!
//! Ours [design, owner 2026-10-15]: behind the menu the plugins' loading screens (LSCR)
//! take turns, in a shuffled order, each held [`HOLD_SECS`] and faded out to black and the
//! next in over [`FADE_SECS`]; the original shows a 3D scene there. The darkened rounded
//! edges are drawn here, not taken from the original's texture.
//!
//! How it fits: the game starts as a new game (the world is Doc Mitchell's house) with the
//! menu over it; while it is open the game is held as under the pause menu
//! (`crate::pause::Paused`: no scripts, the clock and the picture stand still), so New
//! Game plays the intro film (`crate::intro`) and then only lets the opening begin. Load Game and Settings are the pause menu's own pages
//! (`crate::pause`), drawn over this menu; going back from them shows it again. Continue
//! and loading a save start NVForever again with `--load` (`crate::save::LoadRequest`).
//!
//! **The theme**: the original's main menu music, the track its INI names
//! (`SMainMenuMusicTrack=special\maintitle.mp3`) [documented: Fallout_default.ini], read
//! from the owner's `Data\Music` and played from the start, looping [hypothesis: the
//! original loops it], at the file's own level times the music volume
//! (`crate::music::MusicSettings`; silent with `--no-music`) and the Music slider. The
//! original plays it at 0.61 of the file with its Music slider at 60 and
//! `fMainMenuMusicVolume=0.6` [observed: loopback recording fitted to the file, issue
//! #1450]: one 0.6, not both, so the slider (default 0.6) is the theme's only level
//! [hypothesis: the slider and not the INI value; docs/spec-notes/audio.md]. The place's own music stays
//! silent while the menu is open; New Game fades the theme out over [`THEME_FADE`].
//!
//! The INI's main menu colour (`iSystemColorMainMenu…` 199, 255, 165) is not used: the
//! owner's screenshot shows the entries in the interface's amber.
//!
//! **The look is the pause menu's** (`crate::start_menu`) [owner, todo 5 items 8 and 46]:
//! every entry that can be taken at full strength, a greyed one at half, the highlight
//! box round the chosen one; the pointer coming onto an entry and the keys moving the
//! choice play `UIMenuFocus`, taking one `UIMenuOK` [observed: list_box_template.xml
//! `mouseoversound`, `clicksound`]. While a page of the pause menu is up (Load Game,
//! Settings) the pause menu draws the column itself, dimmed but for that page's entry; a
//! click on that entry again closes the page [owner 2026-10-16].
//!
//! Keys: Up/Down (W/S) choose, Enter or Space take; the pointer chooses, a click takes.
//! Test aid `NVF_PLAY_TEST_MAIN_MENU=<seconds>:<action>[,…]` (`up`, `down`, `pick`, or an
//! entry: `continue`, `new`, `load`, `settings`, `quit`; `shot` saves a screenshot
//! `main_menu.png` in the working folder).
//! Log: `main menu: open (…)`, `main menu: <entry>`, `main menu: slide <texture>`.
//!
//! **Co-op** (`crate::lobby`): Join Lobby is shown only while the Co-op switch (a row of the
//! settings page) is on; off, which is the default, it is not shown and takes no room in the
//! column (`Entry::shown`), so a single-player game's menu is the one it was [design]. Test
//! aid entry: `join`.

use crate::interface::{bc_supported, face, plain_font, set_line, u, Face};
use crate::start_menu::{color, entry_base, highlight_colors, Shade, FONT, ITEM, LINE, RIGHT};
use crate::pause::{AidTime, Page, PageRequest, Paused};
use crate::{PlaySet, PlayState, PlayWorld};
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use nvf_assets::{GameData, TextureCache};
use std::path::PathBuf;

/// Seconds a loading screen stays at full strength, and each half of the change (out to
/// black, then the next in) [design].
pub const HOLD_SECS: f32 = 7.0;
pub const FADE_SECS: f32 = 0.9;
/// The loading screens are dimmed behind the menu so its amber reads over them [design].
const SLIDE_BRIGHTNESS: f32 = 0.55;

const LOGO: &str = "textures\\interface\\main\\main_title.dds";
/// The theme [documented: Fallout_default.ini `SMainMenuMusicTrack`].
const THEME: &str = "music\\special\\maintitle.mp3";
/// Seconds the theme takes to fade out after New Game [design].
pub const THEME_FADE: f32 = 1.5;

/// The logo: its picture's left and top, and its width (the texture is 4 to 1; the
/// lettering fills about three quarters of its width) [design, after the screenshot].
const LOGO_LEFT: f32 = 40.0;
const LOGO_TOP: f32 = 400.0;
const LOGO_WIDTH: f32 = 680.0;

/// Asked for by nvforever `--menu`: open the menu as the game starts.
#[derive(Resource)]
pub struct MainMenuStart;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Continue,
    New,
    Load,
    Settings,
    /// Co-op: the join screen, `crate::lobby`; shown only while the Co-op switch is on.
    JoinLobby,
    Quit,
}

impl Entry {
    pub const ALL: [Entry; 6] = [Entry::Continue, Entry::New, Entry::Load, Entry::Settings, Entry::JoinLobby, Entry::Quit];

    pub fn label(self) -> &'static str {
        match self {
            Entry::Continue => "Continue",
            Entry::New => "New Game",
            Entry::Load => "Load Game",
            Entry::JoinLobby => "Join Lobby",
            Entry::Settings => "Settings",
            Entry::Quit => "Quit",
        }
    }

    fn aid_name(self) -> &'static str {
        match self {
            Entry::Continue => "continue",
            Entry::New => "new",
            Entry::Load => "load",
            Entry::JoinLobby => "join",
            Entry::Settings => "settings",
            Entry::Quit => "quit",
        }
    }

    /// Whether the column shows it with the Co-op switch `coop`: Join Lobby only while the
    /// switch is on, so a single-player game's column has nothing of co-op [design].
    pub fn shown(self, coop: bool) -> bool {
        self != Entry::JoinLobby || coop
    }

    /// Whether it can be taken with `saves` saves in the folder and the Co-op switch `coop`.
    pub fn enabled(self, saves: usize, coop: bool) -> bool {
        self.shown(coop) && (!matches!(self, Entry::Continue | Entry::Load) || saves > 0)
    }
}

/// The next entry that can be taken from `from`, `step` (1 down, -1 up), round the ends.
pub fn step(from: usize, step: i32, saves: usize, coop: bool) -> usize {
    let n = Entry::ALL.len() as i32;
    let mut i = from as i32;
    for _ in 0..n {
        i = (i + step).rem_euclid(n);
        if Entry::ALL[i as usize].enabled(saves, coop) {
            return i as usize;
        }
    }
    from
}

/// Where the slideshow is: the screen shown, and how far into its turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Fading in (0 → 1).
    In(f32),
    Hold(f32),
    /// Fading out (1 → 0); the next comes after.
    Out(f32),
}

impl Phase {
    /// After `dt` seconds; `true` when the next screen is due.
    pub fn step(self, dt: f32) -> (Phase, bool) {
        match self {
            Phase::In(t) if t + dt >= FADE_SECS => (Phase::Hold(0.0), false),
            Phase::In(t) => (Phase::In(t + dt), false),
            Phase::Hold(t) if t + dt >= HOLD_SECS => (Phase::Out(0.0), false),
            Phase::Hold(t) => (Phase::Hold(t + dt), false),
            Phase::Out(t) if t + dt >= FADE_SECS => (Phase::In(0.0), true),
            Phase::Out(t) => (Phase::Out(t + dt), false),
        }
    }

    /// The picture's strength.
    pub fn alpha(self) -> f32 {
        match self {
            Phase::In(t) => (t / FADE_SECS).clamp(0.0, 1.0),
            Phase::Hold(_) => 1.0,
            Phase::Out(t) => 1.0 - (t / FADE_SECS).clamp(0.0, 1.0),
        }
    }
}

#[derive(Resource, Default)]
pub struct MainMenu {
    pub open: bool,
    selected: usize,
    saves: usize,
    /// The Co-op switch as of this frame (`crate::lobby::CoopSwitch`).
    coop: bool,
    newest: Option<PathBuf>,
    /// The loading screens' textures, in the order shown.
    slides: Vec<String>,
    slide: usize,
    phase: Option<Phase>,
    /// The game's font 7, and the entries' face as drawn: bold in the vector font while the
    /// Menu Font row says Sharp (`crate::pause::MenuFont::title`), else font 7.
    raster: Option<Face>,
    face: Option<Face>,
    theme: Option<Theme>,
    logo_aspect: Option<f32>,
    logo_hidden: bool,
}

/// Optional Android cover admission. Keep the existing menu until Java's own
/// request/epoch cover frame commits; desktop routes have no such resource.
#[derive(Resource, Default)]
pub struct NewGameCover { requested: bool, ready: bool, active: bool }
impl NewGameCover {
    pub fn requested(&self) -> bool { self.requested }
    pub fn active(&self) -> bool { self.active }
    pub fn allow_start(&mut self) { if self.requested { self.ready = true; } }
    pub fn finish(&mut self) { *self = Self::default(); }
    pub fn request(&mut self) { self.requested = true; self.active = true; }
}

/// The theme playing: its level (changed while it plays) and how loud it is now.
struct Theme {
    levels: std::sync::Arc<[std::sync::atomic::AtomicU32; 3]>,
    entity: Entity,
    level: f32,
}

/// `NVF_PLAY_TEST_MAIN_MENU`: (seconds, action), soonest first.
#[derive(Resource, Default)]
struct MenuAid(Vec<(f32, String)>);

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct Slide;
/// The logo, hidden while a page of the pause menu is up [design: it showed through the
/// settings' list].
#[derive(Component)]
struct Logo;
#[derive(Component, Clone, Copy)]
struct EntryNode(usize);
/// The highlight box round an entry's words (as the pause menu's column, `crate::pause`).
#[derive(Component)]
struct EntryBox;
/// The picture of the save Continue loads, under the logo while Continue is chosen (the
/// player's Continue Shows Its Save tweak) [design: docs/design/tweaks.md].
#[derive(Component)]
struct ContinuePicture;

/// The Continue picture's place and width in the menu's units, under the logo [design].
const PICTURE_TOP: f32 = 620.0;
const PICTURE_WIDTH: f32 = 360.0;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        let mut aid = Vec::new();
        if let Ok(v) = std::env::var("NVF_PLAY_TEST_MAIN_MENU") {
            for part in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part.split_once(':').and_then(|(t, a)| Some((t.trim().parse::<f32>().ok()?, a.trim().to_string()))) {
                    Some(x) => aid.push(x),
                    None => warn!("NVF_PLAY_TEST_MAIN_MENU: {part:?}: expected <seconds>:<action>"),
                }
            }
            aid.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        app.init_resource::<MainMenu>().insert_resource(MenuAid(aid)).add_systems(Update, (open_at_start, follow_coop, choose, slideshow, relabel, show_entries, continue_picture, theme).chain().in_set(PlaySet::Ui));
    }
}

impl MainMenu {
    /// The entries shown, with their labels and whether each can be taken (the pause menu
    /// draws them under its pages): not Join Lobby while the Co-op switch is off.
    pub fn column(&self) -> Vec<(&'static str, bool)> {
        Entry::ALL.iter().filter(|e| e.shown(self.coop)).map(|e| (e.label(), e.enabled(self.saves, self.coop))).collect()
    }
}

/// The Co-op switch as the menu shows it.
fn follow_coop(mut menu: ResMut<MainMenu>, switch: Option<Res<crate::lobby::CoopSwitch>>) {
    let on = switch.is_some_and(|s| s.on);
    if menu.coop != on {
        menu.coop = on;
    }
}

/// Run condition: the main menu is open.
pub fn main_menu_open(m: Option<Res<MainMenu>>) -> bool {
    m.is_some_and(|m| m.open)
}

#[allow(clippy::too_many_arguments)]
fn open_at_start(
    mut commands: Commands,
    start: Option<Res<MainMenuStart>>,
    mut menu: ResMut<MainMenu>,
    mut paused: ResMut<Paused>,
    mut next: ResMut<NextState<PlayState>>,
    cfg: Res<crate::save::SaveConfig>,
    play: Res<PlayWorld>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut music_assets: ResMut<Assets<crate::music::LayeredMusic>>,
    mut menu_font: Option<ResMut<crate::pause::MenuFont>>,
    mut fonts: Option<ResMut<Assets<bevy::text::Font>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if start.is_none() {
        return;
    }
    commands.remove_resource::<MainMenuStart>();
    let list = cfg.dir.as_deref().map(fnv_save::list).unwrap_or_default();
    menu.saves = list.len();
    menu.newest = list.first().map(|(p, _)| p.clone());
    // The screens flagged Displays In Main Menu (docs/spec-notes/loading-screens.md §5).
    let screens = fnv_world::loadscreen::load_screens(play.0.lo, play.0.schema);
    let mut slides = fnv_world::loadscreen::main_menu_pictures(&screens);
    info!("main menu: {} loading screens flagged Displays In Main Menu of {}", screens.iter().filter(|s| s.main_menu).count(), screens.len());
    shuffle(&mut slides);
    menu.slides = slides;
    menu.slide = 0;
    menu.phase = Some(Phase::In(0.0));
    menu.selected = if menu.saves > 0 { 0 } else { 1 };
    let bc = bc_supported(formats.as_deref());
    let raster = face(FONT, plain_font(36.0, 17.0), data.as_deref(), &mut images, bc);
    menu.face = Some(match menu_font.as_deref_mut() {
        Some(m) => m.title(&raster, fonts.as_deref_mut()),
        None => raster.clone(),
    });
    menu.raster = Some(raster);
    let logo = data.as_deref().and_then(|d| crate::interface::texture(LOGO, d, &mut images, bc));
    let edge = images.add(vignette());
    let theme = data.as_deref().and_then(|d| match d.vfs.read(THEME) {
        Some(Ok(b)) => crate::music::LayeredMusic::single(std::sync::Arc::from(b), true),
        Some(Err(e)) => {
            warn!("main menu: {THEME}: {e}");
            None
        }
        None => {
            warn!("main menu: {THEME}: not in the game's files");
            None
        }
    });
    if let Some((music, levels)) = theme {
        let entity = commands.spawn((AudioPlayer::<crate::music::LayeredMusic>(music_assets.add(music)), PlaybackSettings::DESPAWN, crate::audio_out::SoundCategory::Music)).id();
        menu.theme = Some(Theme { levels, entity, level: 0.0 });
        info!("main menu: theme {THEME}");
    }
    menu.logo_aspect = logo.as_ref().map(|(_, s)| s.y / s.x.max(1.0));
    menu.open = true;
    paused.0 = true;
    next.set(PlayState::Menu);
    let shade = images.add(shade());
    let view = windows.single().ok().map_or(Vec2::new(1280.0, 720.0), |w| Vec2::new(w.width(), w.height()));
    if crate::interface::menu_scale() > 1.0 {
        let max_w = menu.face.as_ref().map_or(240.0, max_entry_width);
        menu.logo_hidden = !fit_layout(crate::interface::menu_scale(), view, max_w, shown_count(menu.coop)).show_logo;
    } else {
        menu.logo_hidden = false;
    }
    spawn(&mut commands, &menu, logo, edge, shade, view);
    info!("main menu: open ({} loading screens, {} saves{})", menu.slides.len(), menu.saves, menu.newest.as_ref().map(|p| format!(", newest {}", p.display())).unwrap_or_default());
}

/// A shuffled order, different each start.
pub(crate) fn shuffle(v: &mut [String]) {
    let mut s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
    for i in (1..v.len()).rev() {
        // xorshift64
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        v.swap(i, (s % (i as u64 + 1)) as usize);
    }
}

/// Darkened rounded edges: black, its alpha rising towards the edges of a rounded
/// rectangle [design].
fn vignette() -> Image {
    const W: u32 = 320;
    const H: u32 = 180;
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            // Distance outside a rounded rectangle inset from the edges, in pixels.
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (inset, radius) = (12.0, 40.0);
            let dx = (inset + radius - px).max(px - (W as f32 - inset - radius)).max(0.0);
            let dy = (inset + radius - py).max(py - (H as f32 - inset - radius)).max(0.0);
            let outside = (dx * dx + dy * dy).sqrt() - radius;
            let a = ((outside + 22.0) / 40.0).clamp(0.0, 1.0);
            let a = a * a * (3.0 - 2.0 * a);
            // Darkest at the very edge, about two thirds black [design: the owner found the
            // first, near-black corners too strong, 2026-10-15].
            data.extend_from_slice(&[0, 0, 0, (a * 165.0) as u8]);
        }
    }
    Image::new(Extent3d { width: W, height: H, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

/// A soft shade behind the column of entries: clear at the left, about half black at
/// the right edge, so greyed entries still read over a bright screen [design].
fn shade() -> Image {
    const W: u32 = 64;
    let mut data = Vec::with_capacity((W * 4) as usize);
    for x in 0..W {
        let t = x as f32 / (W - 1) as f32;
        let a = t * t * (3.0 - 2.0 * t) * 0.5;
        data.extend_from_slice(&[0, 0, 0, (a * 255.0) as u8]);
    }
    Image::new(Extent3d { width: W, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

/// [design: docs/design/android.md] When menu scale exceeds 100% (Android up to 200%),
/// clamp and center the main-menu column and logo inside the live viewport so no option
/// is pushed off-screen. Scales <= 100% keep desktop's exact `in_menu_frame` geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FitLayout {
    draw_scale: f32,
    entry_top: f32,
    entry_right: f32,
    logo_left: f32,
    logo_top: f32,
    logo_width: f32,
    picture_top: f32,
    picture_width: f32,
    show_logo: bool,
}

fn max_entry_width(f: &Face) -> f32 {
    Entry::ALL
        .iter()
        .map(|e| f.font.width(fnv_util::words::tr(e.label())) + 2.0 * ENTRY_PAD)
        .fold(0.0_f32, f32::max)
}

fn fit_layout(scale: f32, view: Vec2, max_entry_w: f32, rows: usize) -> FitLayout {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let aspect = if view.x.is_finite() && view.y.is_finite() && view.y > 0.0 {
        (view.x / view.y).clamp(0.4, 4.0)
    } else {
        16.0 / 9.0
    };
    let col_h = rows as f32 * ITEM;
    let max_h_scale = 960.0 / (col_h + 48.0);
    let max_w_scale = (960.0 * aspect) / (max_entry_w + 32.0).max(120.0);
    let draw_scale = s.min(max_h_scale).min(max_w_scale).max(0.5);
    let avail_h = 960.0 / draw_scale;
    let avail_w = 960.0 * aspect / draw_scale;
    let entry_top = ((avail_h - col_h) * 0.5).max(16.0);
    let entry_right = RIGHT.min(((avail_w - max_entry_w) * 0.5).max(8.0));
    let left_room = avail_w - max_entry_w - entry_right - 40.0;
    let show_logo = left_room >= 140.0;
    let logo_left = 24.0_f32.min((left_room * 0.1).max(8.0));
    let logo_width = LOGO_WIDTH.min((left_room - logo_left).max(0.0));
    let logo_top = ((avail_h - logo_width * 0.25) * 0.45).max(16.0);
    let picture_width = PICTURE_WIDTH.min(logo_width);
    let picture_top = (logo_top + logo_width * 0.25 + 24.0).min((avail_h - picture_width * 0.6 - 16.0).max(logo_top));
    FitLayout { draw_scale, entry_top, entry_right, logo_left, logo_top, logo_width, picture_top, picture_width, show_logo }
}

fn spawn(commands: &mut Commands, menu: &MainMenu, logo: Option<(Handle<Image>, Vec2)>, edge: Handle<Image>, shade: Handle<Image>, view: Vec2) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let scale = crate::interface::menu_scale();
    commands
        .spawn((MenuRoot, full.clone(), BackgroundColor(Color::BLACK), GlobalZIndex(100)))
        .with_children(|root| {
            root.spawn((Slide, ImageNode { color: Color::srgba(1.0, 1.0, 1.0, 0.0), ..default() }, Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden));
            // Stretched to the window whatever its shape.
            root.spawn((ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(edge) }, full.clone()));
            root.spawn((ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(shade) }, Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(40.0), height: Val::Percent(100.0), ..default() }));
            if scale > 1.0 {
                let max_w = menu.face.as_ref().map_or(240.0, max_entry_width);
                let fit = fit_layout(scale, view, max_w, shown_count(menu.coop));
                let _units = crate::interface::units_at(fit.draw_scale);
                if let Some((image, size)) = logo {
                    let h = fit.logo_width * size.y / size.x.max(1.0);
                    let vis = if fit.show_logo { Visibility::Inherited } else { Visibility::Hidden };
                    root.spawn((Logo, ImageNode::new(image), Node { position_type: PositionType::Absolute, left: u(fit.logo_left), top: u(fit.logo_top), width: u(fit.logo_width), height: u(h), ..default() }, vis));
                }
                root.spawn((ContinuePicture, ImageNode::default(), Node { position_type: PositionType::Absolute, left: u(fit.logo_left), top: u(fit.picture_top), width: u(fit.picture_width), ..default() }, Visibility::Hidden));
                let Some(f) = menu.face.as_ref() else { return };
                for (i, e) in Entry::ALL.iter().enumerate() {
                    root.spawn((EntryNode(i), Button, entry_node_at(f, i, fit.entry_top, fit.entry_right, menu.coop), BorderColor::all(Color::NONE), BackgroundColor(Color::NONE))).with_children(|p| entry_label(p, f, *e));
                }
            } else {
                // The logo and the entries at the UI scale, in the middle of the view
                // (`crate::interface::menu_frame`, #1631); the pictures above fill the view.
                crate::interface::in_menu_frame(root, crate::interface::MIDDLE, |root| {
                    if let Some((image, size)) = logo {
                        let h = LOGO_WIDTH * size.y / size.x.max(1.0);
                        root.spawn((Logo, ImageNode::new(image), Node { position_type: PositionType::Absolute, left: u(LOGO_LEFT), top: u(LOGO_TOP), width: u(LOGO_WIDTH), height: u(h), ..default() }));
                    }
                    root.spawn((ContinuePicture, ImageNode::default(), Node { position_type: PositionType::Absolute, left: u(LOGO_LEFT), top: u(PICTURE_TOP), width: u(PICTURE_WIDTH), ..default() }, Visibility::Hidden));
                    let Some(f) = menu.face.as_ref() else { return };
                    for (i, e) in Entry::ALL.iter().enumerate() {
                        // The box round the words takes the pointer, not the rest of the row
                        // (`entry_node`); its highlight box is lit by `show_entries`.
                        root.spawn((EntryNode(i), Button, entry_node(f, i, menu.coop), BorderColor::all(Color::NONE), BackgroundColor(Color::NONE))).with_children(|p| entry_label(p, f, *e));
                    }
                });
            }
        });
}

/// An entry's label in the language chosen, right-aligned in its row, and the highlight box
/// that hugs it (lit by `show_entries`), where the pause menu's column has its words and box
/// (`crate::pause`, 12 units each side) so the words stay put as its pages open.
fn entry_label(p: &mut ChildSpawnerCommands, f: &Face, e: Entry) {
    let label = fnv_util::words::tr(e.label());
    if let Some(line) = f.font.lines(label, None).into_iter().next() {
        // The box with its glow (`crate::menu_shapes::framed`), filling the entry's node,
        // shown while the entry is chosen.
        let (line_colour, fill) = highlight_colors();
        p.spawn((EntryBox, crate::menu_shapes::filling(), Visibility::Hidden, Pickable::IGNORE)).with_children(|c| crate::menu_shapes::framed(c, line_colour, fill, LINE));
        set_line(p, f, &line, ENTRY_PAD, entry_base(f), color(Shade::Bright));
    }
}

/// The room each side of an entry's words in its box (the pause menu's column's,
/// `crate::pause`).
const ENTRY_PAD: f32 = 12.0;

/// Entry `i`'s node: the box round its words, right-aligned as the pause menu's column's
/// entries (their words end `ENTRY_PAD` before the column's edge), and only it takes the
/// pointer [design: owner 2026-11-16, "the main menu titles show the hover box even when you
/// hover some distance from the box ... make the activation boxes the same size as the menu
/// hover boxes"].
fn entry_node(f: &Face, i: usize, coop: bool) -> Node {
    let top = (960.0 - shown_count(coop) as f32 * ITEM) / 2.0;
    entry_node_at(f, i, top, RIGHT, coop)
}

fn entry_node_at(f: &Face, i: usize, column_top: f32, right: f32, coop: bool) -> Node {
    let top = column_top + slot(i, coop) as f32 * ITEM;
    let w = f.font.width(fnv_util::words::tr(Entry::ALL[i].label())) + 2.0 * ENTRY_PAD;
    // An entry that is not shown takes no room in the column.
    let display = if Entry::ALL[i].shown(coop) { Display::Flex } else { Display::None };
    Node { position_type: PositionType::Absolute, display, right: u(right), top: u(top), width: u(w), height: u(ITEM), ..default() }
}

/// Where entry `i` stands in the column, counting only the entries shown with the Co-op
/// switch `coop`.
fn slot(i: usize, coop: bool) -> usize {
    Entry::ALL[..i].iter().filter(|e| e.shown(coop)).count()
}

/// How many entries the column shows with the Co-op switch `coop`.
fn shown_count(coop: bool) -> usize {
    Entry::ALL.iter().filter(|e| e.shown(coop)).count()
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RelabelKey {
    words: u32,
    sharp: bool,
    cjk: u32,
    dims: (u32, u32),
    /// The Co-op switch: whether Join Lobby is in the column.
    coop: bool,
}

/// The entries' labels drawn again in the new language, or in the new face when the Menu Font
/// row changed (both chosen on the settings page, which opens over this menu), or a CJK fallback
/// font is ready, or when the live viewport changes at a scale above 100%.
#[allow(clippy::too_many_arguments)]
fn relabel(
    mut menu: ResMut<MainMenu>,
    mut seen: Local<Option<RelabelKey>>,
    mut menu_font: Option<ResMut<crate::pause::MenuFont>>,
    mut fonts: Option<ResMut<Assets<bevy::text::Font>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    entries: Query<(Entity, &EntryNode)>,
    mut logos: Query<(&mut Node, &mut Visibility), With<Logo>>,
    mut commands: Commands,
) {
    let sharp = menu_font.as_deref().is_some_and(crate::pause::MenuFont::drawn_sharp);
    let scale = crate::interface::menu_scale();
    let view = windows.single().ok().map_or(Vec2::new(1280.0, 720.0), |w| Vec2::new(w.width(), w.height()));
    let dims = if scale > 1.0 { (view.x.round() as u32, view.y.round() as u32) } else { (0, 0) };
    let now = RelabelKey { words: fnv_util::words::generation(), sharp, cjk: crate::cjk_font::revision(), dims, coop: menu.coop };
    if *seen == Some(now) {
        return;
    }
    if seen.is_some_and(|s| s.sharp != sharp) {
        if let (Some(raster), Some(m)) = (menu.raster.clone(), menu_font.as_deref_mut()) {
            menu.face = Some(m.title(&raster, fonts.as_deref_mut()));
        }
    }
    let Some(f) = menu.face.as_ref() else { return };
    if entries.is_empty() {
        return;
    }
    if seen.is_some() || scale > 1.0 {
        if scale > 1.0 {
            let fit = fit_layout(scale, view, max_entry_width(f), shown_count(menu.coop));
            let _units = crate::interface::units_at(fit.draw_scale);
            for (id, e) in &entries {
                commands.entity(id).despawn_related::<Children>().insert(entry_node_at(f, e.0, fit.entry_top, fit.entry_right, menu.coop)).with_children(|p| entry_label(p, f, Entry::ALL[e.0]));
            }
            for (mut node, mut vis) in &mut logos {
                node.left = u(fit.logo_left);
                node.top = u(fit.logo_top);
                node.width = u(fit.logo_width);
                if let Some(aspect) = menu.logo_aspect {
                    node.height = u(fit.logo_width * aspect);
                }
                vis.set_if_neq(if fit.show_logo { Visibility::Inherited } else { Visibility::Hidden });
            }
            menu.logo_hidden = !fit.show_logo;
        } else {
            for (id, e) in &entries {
                commands.entity(id).despawn_related::<Children>().insert(entry_node(f, e.0, menu.coop)).with_children(|p| entry_label(p, f, Entry::ALL[e.0]));
            }
        }
    }
    *seen = Some(now);
}

#[allow(clippy::too_many_arguments)]
fn choose(
    keys: crate::pad::ActionButtons<KeyCode>,
    time: AidTime,
    mut aid: ResMut<MenuAid>,
    mut menu: ResMut<MainMenu>,
    mut cover: Option<ResMut<NewGameCover>>,
    page: Res<Page>,
    mut request: ResMut<PageRequest>,
    entries: Query<(&PointerState, &EntryNode), Changed<PointerState>>,
    mut loads: MessageWriter<crate::save::LoadRequest>,
    mut intro: MessageWriter<crate::intro::PlayIntro>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    roots: Query<Entity, With<MenuRoot>>,
    mut last_page: Local<Option<Page>>,
    mut sounds: MessageWriter<crate::ui_sounds::UiSound>,
    (tweaks, mut auto_done, mut lobby): (Option<Res<crate::tweaks::TweakSettings>>, Local<bool>, Option<ResMut<crate::lobby::LobbyScreen>>),
) {
    let keys = keys.snapshot();
    use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
    if !menu.open {
        return;
    }
    // The lobby screen over it takes the keys (`crate::lobby`).
    if lobby.as_ref().is_some_and(|l| l.holds_input()) {
        return;
    }
    // The frame a pause page goes away is not this menu's: its click or key was that page's.
    let came_back = last_page.replace(*page).is_some_and(|p| p != Page::Main);
    let now = time.elapsed_secs();
    let mut acts = Vec::new();
    while aid.0.first().is_some_and(|(t, _)| *t <= now) {
        let (_, a) = aid.0.remove(0);
        info!("test aid NVF_PLAY_TEST_MAIN_MENU: {a}");
        acts.push(a);
    }
    // The pause menu's pages (Load Game, Settings) take the keys while they are up.
    if *page != Page::Main || request.0.is_some() || came_back {
        return;
    }
    let cover_ready = cover.as_ref().is_some_and(|c| c.requested && c.ready);
    if cover.as_ref().is_some_and(|c| c.requested && !c.ready) { return; }
    let (saves, coop) = (menu.saves, menu.coop);
    let before = menu.selected;
    let mut take = None;
    for (i, e) in &entries {
        let ok = Entry::ALL[e.0].enabled(saves, coop);
        match i {
            PointerState::Hovered if ok => menu.selected = e.0,
            PointerState::Pressed if ok => take = Some(Entry::ALL[e.0]),
            _ => {}
        }
    }
    for a in &acts {
        match a.as_str() {
            "up" => menu.selected = step(menu.selected, -1, saves, coop),
            "down" => menu.selected = step(menu.selected, 1, saves, coop),
            "pick" => take = Some(Entry::ALL[menu.selected]),
            "shot" => {
                info!("main menu: screenshot main_menu.png");
                commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk("main_menu.png"));
            }
            other => match Entry::ALL.iter().find(|e| e.aid_name() == other) {
                Some(e) if e.enabled(saves, coop) => take = Some(*e),
                Some(e) => info!("main menu: {} is greyed out", e.label()),
                None => warn!("NVF_PLAY_TEST_MAIN_MENU: no action {other:?}"),
            },
        }
    }
    if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
        menu.selected = step(menu.selected, -1, saves, coop);
    }
    if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
        menu.selected = step(menu.selected, 1, saves, coop);
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) && Entry::ALL[menu.selected].enabled(saves, coop) {
        take = Some(Entry::ALL[menu.selected]);
    }
    if menu.selected != before {
        sounds.write(UiSound { menu: UiMenu::MainMenu, event: UiEvent::Focus });
    }
    // The player's tweak continues the newest save once, at the first menu; Shift held
    // stays on the menu (a way back to it) [design: docs/design/tweaks.md].
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if take.is_none() && !*auto_done && !shift && tweaks.as_ref().is_some_and(|t| t.auto_continue) {
        *auto_done = true;
        if Entry::Continue.enabled(saves, coop) {
            info!("main menu: continuing the newest save (tweak)");
            take = Some(Entry::Continue);
        }
    }
    if cover_ready { take = Some(Entry::New); }
    let Some(e) = take else { return };
    if e == Entry::New {
        if let Some(cover) = cover.as_deref_mut() {
            if !cover.ready { cover.request(); return; }
            cover.requested = false; cover.ready = false;
        }
    }
    info!("main menu: {}", e.label());
    sounds.write(UiSound { menu: UiMenu::MainMenu, event: UiEvent::Picked });
    match e {
        Entry::Continue => {
            if let Some(p) = menu.newest.clone() {
                info!("main menu: continue from {}", p.display());
                loads.write(crate::save::LoadRequest { path: p });
            }
        }
        Entry::New => {
            // The intro film plays first (`crate::intro`), then the held game begins.
            menu.open = false;
            for r in &roots {
                commands.entity(r).despawn();
            }
            intro.write(crate::intro::PlayIntro);
            info!("main menu: closed, the intro film plays");
        }
        Entry::Load => request.0 = Some(Page::Load { selected: 0 }),
        // The switch turns over (and is kept); the entries follow next frame.
        Entry::JoinLobby => match lobby.as_mut() {
            Some(l) => l.open(crate::lobby::Mode::Join),
            None => warn!("main menu: co-op is not in this build"),
        },
        Entry::Settings => request.0 = Some(Page::Settings { selected: None }),
        Entry::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

/// The loading screens behind the menu, one after another with a fade through black.
#[allow(clippy::too_many_arguments)]
fn slideshow(
    real: Res<Time<Real>>,
    mut menu: ResMut<MainMenu>,
    mut slides: Query<(&mut ImageNode, &mut Node, &mut Visibility), With<Slide>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    data: Option<Res<GameData>>,
    textures: Option<ResMut<TextureCache>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
) {
    let Some(phase) = menu.phase.filter(|_| menu.open) else { return };
    let Ok((mut img, mut node, mut vis)) = slides.single_mut() else { return };
    if menu.slides.is_empty() {
        return;
    }
    let (phase, next) = phase.step(real.delta_secs().min(0.1));
    menu.phase = Some(phase);
    if next {
        menu.slide = (menu.slide + 1) % menu.slides.len();
    }
    // A new picture at the start of each turn (and the first).
    let want = &menu.slides[menu.slide];
    let fresh = next || *vis == Visibility::Hidden;
    if fresh {
        if let (Some(d), Some(mut cache)) = (data.as_deref(), textures) {
            match cache.get(want, true, d, &mut images, formats.as_deref()) {
                Some(h) => {
                    img.image = h;
                    *vis = Visibility::Inherited;
                    info!("main menu: slide {want}");
                }
                None => {
                    warn!("main menu: {want}: not shown");
                    menu.slide = (menu.slide + 1) % menu.slides.len();
                }
            }
        }
    }
    img.color = Color::srgba(SLIDE_BRIGHTNESS, SLIDE_BRIGHTNESS, SLIDE_BRIGHTNESS, phase.alpha());
    // Cover the window, cropping the overflow evenly.
    if let (Ok(w), Some(i)) = (windows.single(), images.get(&img.image)) {
        let (size, offset) = crate::loading::cover(i.size().as_vec2(), Vec2::new(w.width(), w.height()));
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
        node.left = Val::Px(offset.x);
        node.top = Val::Px(offset.y);
    }
}

/// Every entry that can be taken bright, a greyed one at half, the highlight box round
/// the chosen one; the column hidden while a page of the pause menu draws it [design].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show_entries(
    menu: Res<MainMenu>,
    page: Res<Page>,
    mut entries: Query<(Entity, &EntryNode, &mut Visibility), Without<EntryBox>>,
    children: Query<&Children>,
    mut boxes: Query<&mut Visibility, (With<EntryBox>, Without<EntryNode>, Without<Logo>)>,
    mut glyphs: Query<&mut ImageNode, Without<Slide>>,
    mut texts: Query<&mut TextColor, Without<crate::interface::Halo>>,
    mut logo: Query<&mut Visibility, (With<Logo>, Without<EntryNode>)>,
    mut drawn: Local<[Option<(Color, usize)>; Entry::ALL.len()]>,
) {
    if !menu.open {
        return;
    }
    for mut v in &mut logo {
        v.set_if_neq(if *page == Page::Main && !menu.logo_hidden { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (id, e, mut vis) in &mut entries {
        let entry = Entry::ALL[e.0];
        let shown = if *page == Page::Main && entry.shown(menu.coop) { Visibility::Inherited } else { Visibility::Hidden };
        vis.set_if_neq(shown);
        let c = color(if entry.enabled(menu.saves, menu.coop) { Shade::Bright } else { Shade::Dim });
        let chosen = e.0 == menu.selected;
        let (mut ink, mut count) = (None, 0);
        // [design, #2247] set_line/set_marked put the coloured nodes below a line
        // anchor (interface.rs); reach them even when the label has extra containers.
        for child in children.iter_descendants(id) {
            if let Ok(mut b) = boxes.get_mut(child) {
                b.set_if_neq(if chosen { Visibility::Inherited } else { Visibility::Hidden });
                continue;
            }
            if let Ok(mut g) = glyphs.get_mut(child) {
                if g.color != c {
                    g.color = c;
                }
                ink = Some(g.color);
                count += 1;
            }
            if let Ok(mut t) = texts.get_mut(child) {
                t.set_if_neq(TextColor(c));
                ink = Some(t.0);
                count += 1;
            }
        }
        // [design, #2247] Report actual nonempty glyph assignments on first draw or
        // a colour/count change, so smoke checks cannot pass from enablement alone.
        let now = ink.map(|colour| (colour, count));
        if drawn[e.0] != now {
            if let Some((colour, count)) = now {
                info!("main menu: {} glyph colour rgba {:?} ({count} glyphs)", entry.label(), colour.to_srgba().to_u8_array());
            }
            drawn[e.0] = now;
        }
    }
}

/// The Continue Shows Its Save tweak: while Continue is the chosen entry on the main page,
/// the newest save's thumbnail under the logo, read once (none for a save without one).
#[allow(clippy::type_complexity)]
fn continue_picture(
    menu: Res<MainMenu>,
    page: Res<Page>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut images: ResMut<Assets<Image>>,
    mut read: Local<Option<(PathBuf, Option<(Handle<Image>, f32)>)>>,
    mut picture: Query<(&mut ImageNode, &mut Node, &mut Visibility), With<ContinuePicture>>,
) {
    let Ok((mut image, mut node, mut vis)) = picture.single_mut() else { return };
    let wanted = menu.open && *page == Page::Main && tweaks.as_ref().is_some_and(|t| t.continue_shows_save) && Entry::ALL.get(menu.selected) == Some(&Entry::Continue);
    let path = menu.newest.as_ref().filter(|_| wanted);
    let Some(path) = path else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    if read.as_ref().is_none_or(|(p, _)| p != path) {
        let decoded = fnv_save::read_header(path).ok().and_then(|f| crate::save::decode_thumbnail(&f.shot));
        let handle = decoded.and_then(|(w, h, rgba)| {
            let format = TextureFormat::Rgba8UnormSrgb;
            let mut img = nvf_assets::texture::checked_image(w, h, 1, format, rgba).map_err(|e| warn!("main menu: the newest save's picture: {e}")).ok()?;
            img.asset_usage = RenderAssetUsages::default();
            Some((images.add(img), h as f32 / (w as f32).max(1.0)))
        });
        info!("main menu: Continue's save picture {}", if handle.is_some() { "read" } else { "missing" });
        *read = Some((path.clone(), handle));
    }
    match read.as_ref().and_then(|(_, h)| h.as_ref()) {
        Some((h, aspect)) => {
            if image.image != *h {
                image.image = h.clone();
                node.height = u(PICTURE_WIDTH * aspect);
            }
            vis.set_if_neq(Visibility::Inherited);
        }
        None => {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
}

/// The theme's level: up to its volume while the menu is open, down to silence after.
fn theme(real: Res<Time<Real>>, mut menu: ResMut<MainMenu>, settings: Option<Res<crate::music::MusicSettings>>, mut commands: Commands) {
    let open = menu.open;
    let Some(t) = menu.theme.as_mut() else { return };
    let volume = settings.as_deref().map_or(1.0, |s| if s.play { s.volume.max(0.0) } else { 0.0 });
    let dt = real.delta_secs().min(0.1);
    t.level = if open { (t.level + dt / 0.5).min(1.0) } else { (t.level - dt / THEME_FADE).max(0.0) };
    t.levels[0].store((t.level * volume).to_bits(), std::sync::atomic::Ordering::Relaxed);
    if !open && t.level <= 0.0 {
        if let Ok(mut e) = commands.get_entity(t.entity) {
            e.despawn();
        }
        menu.theme = None;
        info!("main menu: theme faded out");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [design, #2247] Synthetic font metrics and a dummy handle exercise the real
    /// spawn/entry_label/set_line hierarchy without reading any game files.
    fn colour_app(bitmap: bool) -> (App, Vec<(Entity, Entry, Vec<Entity>)>) {
        let mut app = App::new();
        let glyph = fnv_font::Glyph { uv: [0.0, 0.0, 0.5, 0.75], width: 8.0, height: 12.0, ascent: 10.0, after: 1.0, ..default() };
        let face = Face {
            font: fnv_font::Font { size: 16.0, texture: String::new(), glyphs: vec![glyph; 256], extra: Default::default() },
            image: bitmap.then(|| (Handle::default(), Vec2::splat(16.0))),
            vector: None,
        };
        let menu = MainMenu { open: true, selected: 1, face: Some(face), ..default() };
        spawn(&mut app.world_mut().commands(), &menu, None, Handle::default(), Handle::default(), Vec2::new(1280.0, 720.0));
        app.world_mut().flush();
        app.insert_resource(menu).init_resource::<Page>().add_systems(Update, show_entries);
        let world = app.world_mut();
        let entries: Vec<_> = world.query::<(Entity, &EntryNode)>().iter(world).map(|(id, e)| (id, Entry::ALL[e.0])).collect();
        assert_eq!(entries.len(), Entry::ALL.len());
        let entries = entries.into_iter().map(|(id, entry)| {
            let anchors = world.get::<Children>(id).unwrap();
            let mut glyphs = Vec::new();
            for anchor in anchors.iter().filter(|a| world.get::<EntryBox>(*a).is_none()) {
                assert!(world.get::<ImageNode>(anchor).is_none() && world.get::<TextColor>(anchor).is_none());
                glyphs.extend(world.get::<Children>(anchor).unwrap().iter());
            }
            assert!(!glyphs.is_empty(), "{} has ink below its anchor", entry.label());
            (id, entry, glyphs)
        }).collect();
        (app, entries)
    }

    fn check_entry_colours(bitmap: bool) {
        let (mut app, entries) = colour_app(bitmap);
        for saves in [0, 1, 0] {
            app.world_mut().resource_mut::<MainMenu>().saves = saves;
            app.update();
            for (id, entry, glyphs) in &entries {
                let expected = color(if entry.enabled(saves, false) { Shade::Bright } else { Shade::Dim });
                for glyph in glyphs {
                    let actual = if bitmap {
                        app.world().get::<ImageNode>(*glyph).unwrap().color
                    } else {
                        app.world().get::<TextColor>(*glyph).unwrap().0
                    };
                    assert_eq!(actual, expected, "{} with {saves} saves", entry.label());
                }
                let (line, fill) = highlight_colors();
                let selected = *entry == Entry::New;
                // The box hugging the words shows on the chosen entry only, the row itself is
                // never lit.
                let boxed = app.world().get::<Children>(*id).unwrap().iter().find(|c| app.world().get::<EntryBox>(*c).is_some()).expect("the entry's box");
                assert_eq!(*app.world().get::<Visibility>(boxed).unwrap(), if selected { Visibility::Inherited } else { Visibility::Hidden });
                let lines: Vec<(BorderColor, Color)> = app.world().get::<Children>(boxed).unwrap().iter().filter(|c| app.world().get::<crate::menu_shapes::LineBox>(*c).is_some()).map(|c| (*app.world().get::<BorderColor>(c).unwrap(), app.world().get::<BackgroundColor>(c).unwrap().0)).collect();
                assert_eq!(lines, [(BorderColor::all(line), fill)]);
                assert_eq!(*app.world().get::<BorderColor>(*id).unwrap(), BorderColor::all(Color::NONE));
            }
        }
        *app.world_mut().resource_mut::<Page>() = Page::Settings { selected: None };
        app.update();
        for (id, _, _) in &entries {
            assert_eq!(*app.world().get::<Visibility>(*id).unwrap(), Visibility::Hidden);
        }
        *app.world_mut().resource_mut::<Page>() = Page::Main;
        app.update();
        // With the Co-op switch off, Join Lobby is not shown at all.
        for (id, entry, _) in &entries {
            let want = if entry.shown(false) { Visibility::Inherited } else { Visibility::Hidden };
            assert_eq!(*app.world().get::<Visibility>(*id).unwrap(), want, "{}", entry.label());
        }
    }

    #[test]
    fn bitmap_entry_glyphs_dim_without_saves_and_brighten_when_a_save_appears() {
        check_entry_colours(true);
    }

    #[test]
    fn fallback_entry_text_dims_without_saves_and_brightens_when_a_save_appears() {
        check_entry_colours(false);
    }

    #[test]
    fn without_saves_continue_and_load_are_skipped() {
        // Co-op off: Join Lobby is not shown, so it is skipped too.
        assert!(!Entry::Continue.enabled(0, false) && !Entry::Load.enabled(0, false));
        assert!(Entry::New.enabled(0, false) && Entry::Continue.enabled(1, false));
        // New Game (1) down: Load is greyed, so Settings (3); up from New: round to Quit.
        assert_eq!(step(1, 1, 0, false), 3);
        assert_eq!(step(1, -1, 0, false), 5);
        assert_eq!(step(1, -1, 2, false), 0);
        assert_eq!(step(5, 1, 0, false), 1);
    }

    /// Co-op off (the default): Join Lobby is not shown, takes no room in the column and is
    /// skipped; the column is what a game without co-op has, Settings at index 3. On, Join
    /// Lobby stands after Settings and can be taken.
    #[test]
    fn join_lobby_is_not_shown_while_co_op_is_off() {
        assert!(!MainMenu::default().coop, "off by default");
        assert!(!Entry::JoinLobby.shown(false) && Entry::JoinLobby.shown(true));
        assert_eq!(shown_count(false), Entry::ALL.len() - 1);
        assert_eq!(shown_count(true), Entry::ALL.len());
        assert_eq!(Entry::ALL[3], Entry::Settings);
        // Quit takes Join Lobby's place while it is hidden.
        assert_eq!(slot(5, false), 4);
        assert_eq!(slot(5, true), 5);
        for saves in [0, 3] {
            assert!(!Entry::JoinLobby.enabled(saves, false));
        }
        assert!(Entry::JoinLobby.enabled(0, true));
        // Off: Settings (3) down skips Join Lobby to Quit; on, Join Lobby is next.
        assert_eq!(step(3, 1, 3, false), 5);
        assert_eq!(step(5, -1, 3, false), 3);
        assert_eq!(step(3, 1, 3, true), 4);
        let off = MainMenu { saves: 3, ..default() }.column();
        assert_eq!(off.len(), Entry::ALL.len() - 1);
        assert_eq!(off[3], ("Settings", true));
        assert!(!off.iter().any(|c| c.0 == "Join Lobby"), "Join Lobby is not drawn with co-op off");
        let on = MainMenu { saves: 3, coop: true, ..default() }.column();
        assert_eq!(on[4], ("Join Lobby", true));
    }

    #[test]
    fn a_slide_holds_then_fades_out_and_the_next_fades_in() {
        let mut p = Phase::In(0.0);
        let mut changes = 0;
        let mut t = 0.0;
        while t < 2.0 * (HOLD_SECS + 2.0 * FADE_SECS) {
            let (q, next) = p.step(0.05);
            changes += next as u32;
            p = q;
            t += 0.05;
        }
        assert_eq!(changes, 2);
        assert_eq!(Phase::Hold(1.0).alpha(), 1.0);
        assert!((Phase::Out(FADE_SECS / 2.0).alpha() - 0.5).abs() < 1e-6);
    }
    #[test]
    fn new_game_keeps_the_actual_menu_until_cover_commit_and_emits_intro_once() {
        use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState, InputPlugin};
        for platform_cover in [false, true] {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, InputPlugin))
                .insert_resource(MainMenu { open: true, selected: 1, ..default() })
                .init_resource::<Page>().init_resource::<PageRequest>()
                .init_resource::<crate::pause::PlayClock>().init_resource::<MenuAid>()
                .add_message::<crate::save::LoadRequest>().add_message::<crate::intro::PlayIntro>()
                .add_message::<crate::ui_sounds::UiSound>().add_systems(Update, choose);
            if platform_cover { app.init_resource::<NewGameCover>(); }
            let root = app.world_mut().spawn(MenuRoot).id();
            let mut reader = bevy::ecs::message::MessageCursor::<crate::intro::PlayIntro>::default();
            app.world_mut().write_message(KeyboardInput { key_code: KeyCode::Enter, logical_key: Key::Enter,
                state: ButtonState::Pressed, repeat: false, text: None, window: Entity::PLACEHOLDER });
            app.update();
            let emitted = reader.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count();
            if platform_cover {
                assert_eq!(emitted, 0);
                assert!(app.world().resource::<MainMenu>().open && app.world().get_entity(root).is_ok());
                assert!(app.world().resource::<NewGameCover>().requested());
                app.update();
                assert_eq!(reader.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 0);
                app.world_mut().resource_mut::<NewGameCover>().allow_start(); app.update();
                assert_eq!(reader.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 1);
            } else { assert_eq!(emitted, 1, "ordinary desktop admission remains immediate"); }
            assert!(!app.world().resource::<MainMenu>().open && app.world().get_entity(root).is_err());
            app.update();
            assert_eq!(reader.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 0);
        }
    }

    #[test]
    fn auto_continue_cannot_bypass_the_actual_pending_new_game_cover_or_duplicate_the_intro() {
        use bevy::input::InputPlugin;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin))
            .insert_resource(MainMenu { open: true, saves: 1, newest: Some(PathBuf::from("authored-save.fos")), ..default() })
            .insert_resource(crate::tweaks::TweakSettings { auto_continue: true, ..default() })
            .init_resource::<Page>().init_resource::<PageRequest>()
            .init_resource::<crate::pause::PlayClock>().init_resource::<MenuAid>()
            .init_resource::<NewGameCover>()
            .add_message::<crate::save::LoadRequest>().add_message::<crate::intro::PlayIntro>()
            .add_message::<crate::ui_sounds::UiSound>().add_systems(Update, choose);
        app.world_mut().resource_mut::<NewGameCover>().request();
        let root = app.world_mut().spawn(MenuRoot).id();
        let mut loads = bevy::ecs::message::MessageCursor::<crate::save::LoadRequest>::default();
        let mut intros = bevy::ecs::message::MessageCursor::<crate::intro::PlayIntro>::default();
        for _ in 0..2 {
            app.update();
            assert_eq!(loads.read(app.world().resource::<Messages<crate::save::LoadRequest>>()).count(), 0);
            assert_eq!(intros.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 0);
            assert!(app.world().resource::<MainMenu>().open && app.world().get_entity(root).is_ok());
        }
        app.world_mut().resource_mut::<NewGameCover>().allow_start(); app.update();
        assert_eq!(loads.read(app.world().resource::<Messages<crate::save::LoadRequest>>()).count(), 0);
        assert_eq!(intros.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 1);
        assert!(!app.world().resource::<MainMenu>().open && app.world().get_entity(root).is_err());
        app.update();
        assert_eq!(loads.read(app.world().resource::<Messages<crate::save::LoadRequest>>()).count(), 0);
        assert_eq!(intros.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 0);
    }

    #[test]
    fn main_menu_fit_keeps_all_entries_inside_viewport_at_high_scales() {
        let face = Face { font: plain_font(36.0, 17.0), image: None, vector: None };
        let max_w = max_entry_width(&face);
        for view in [Vec2::new(880.0, 396.0), Vec2::new(1280.0, 720.0), Vec2::new(393.0, 873.0)] {
            for scale in [1.2, 1.5, 2.0] {
                let fit = fit_layout(scale, view, max_w, Entry::ALL.len());
                let px_per_u = view.y * fit.draw_scale / 960.0;
                let col_top_px = fit.entry_top * px_per_u;
                let col_bottom_px = (fit.entry_top + Entry::ALL.len() as f32 * ITEM) * px_per_u;
                let right_px = fit.entry_right * px_per_u;
                let widest_left_px = view.x - right_px - max_w * px_per_u;
                assert!(col_top_px >= 0.0 && col_bottom_px <= view.y + 0.5,
                    "vertical bounds [{col_top_px}, {col_bottom_px}] exceed {view} at {scale}x");
                assert!(widest_left_px >= -0.5 && right_px >= 0.0,
                    "horizontal bounds left={widest_left_px}, right={right_px} exceed {view} at {scale}x");
            }
        }
    }

    include!("main_menu_retained_cursor_tests.rs");

}
