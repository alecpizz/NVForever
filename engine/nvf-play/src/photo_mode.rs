//! Photo mode (docs/design/photo-mode.md) [design]: a
//! camera that leaves the player to frame a picture, a few sliders, and a key that saves
//! the view as a PNG. Its feel and layout follow Fallout 76's photo mode [design; nothing
//! of 76's art is used]: the HUD and the crosshair go, and a
//! small panel sits at the right in the HUD's font and colour over a soft dark backdrop;
//! it can be hidden for a clean view.
//!
//! **The camera** is free by default, not locked to the player [design]: it looks with the mouse and flies anywhere within [`MAX_FREE_RANGE`] of the
//! player, walls stopping it. Its Orbit mode is the first version's camera: it circles the
//! player and moves a short way off ([`MAX_PAN`]).
//!
//! **The options** ([`PhotoSettings`]), on three tabs as photo modes commonly group them
//! [design]:
//! Camera (camera mode, zoom, roll, depth of field's focus and blur), Look (brightness,
//! contrast, saturation, warmth, vignette, film grain) and Frame (letterbox bars, a
//! rule-of-thirds grid that is never in the photo, hide the player). The look and the
//! depth of field go through nvforever's image-space grade (`imagespace.rs`,
//! [`PhotoMode::look`], [`PhotoMode::finish`], [`PhotoMode::depth_of_field`]), so the
//! photo is graded as the screen is. Every option goes back to its default when photo mode
//! closes.
//!
//! **Opening and closing.** Photo Mode (`crate::bindings::Control::PhotoMode`, the one
//! binding photo mode has; unbound by default, no key and no controller button, #3634)
//! opens it in the world with no menu, console or loading screen up; the pause menu's
//! Photo Mode entry does too, which is how it opens until the player binds it.
//! Esc, Photo Mode again, or B on a controller (Start, which is Esc) close it, and the view
//! goes back to the player as it was (first or third person, the field of view).
//!
//! **The game pauses** while it is open: the world stands still as under a menu
//! (`PlayState::Menu`) and so does the picture (the virtual clock,
//! `crate::pause::freeze_picture`).
//!
//! **The picture** ([`take`]): the panel, the corner messages and every other piece of the
//! interface are hidden for one frame and the window is captured at its full size, then
//! written as a PNG on its own thread to `<NVForever home>\Photos` ([`photos_dir`]), named by
//! the local date and time and the place ([`photo_file_name`]): "NVForever 2026-11-10
//! 01-23-45 Goodsprings.png". "Photo saved" is photo mode's own notice then, low in the
//! middle of the view ([`NOTICE_SECONDS`]); the corner messages wait, hidden and with their
//! clocks stopped, until photo mode closes, as behind a menu (`crate::corner_messages`)
//! [design: they are part of the HUD, which photo mode hides].
//!
//! **Controls** [design]. Photo mode's keys and buttons are its own and fixed, but for
//! the Photo Mode binding that opens it ([`PhotoKey`]). They
//! are read only while photo mode is open, which holds the play controls meanwhile
//! (`ControlInput::suppressed`). Keyboard and mouse: the Photo Mode binding opens and closes it, the mouse looks
//! (Orbit: circles), the wheel zooms, W/S fly forward and back along the view (Orbit:
//! nearer and farther), A/D sideways, Space or E up, Ctrl or Q down, Shift faster; Tab
//! (Shift+Tab) changes the tab, Up/Down choose an option, Left/Right change it; Enter or the
//! left button takes the photo, H hides the panel, R resets the options, Esc closes. A
//! controller: the right stick looks, the left stick flies, the triggers lower and raise
//! it, LB/RB change the tab; the D-pad chooses and changes an option; A takes the photo, Y
//! hides the panel, X resets, B closes.
//!
//! Test aid `NVF_PLAY_TEST_PHOTO="<seconds>:<action>,..."` (on the aids' clock): `open`,
//! `close`, `shot` (take a photo), `hide`, `show` (the panel), `orbit=<degrees>` (turn
//! the camera; Orbit: circle), `near=<units>` (Orbit: its distance), `lift=<units>` and
//! `fly=<units>` (the free camera rises, then flies along the view), `face` (the free
//! camera turns to the player), `set=<option>=<value>` ([`Setting::key`]: degrees
//! for zoom and roll, the place 0-100 for focus, an index for a choice, a factor for the
//! rest), `tab=<n>`, `select=<row>`, `screen=<file>` (a screenshot of the window as it is,
//! with the panel, for the docs).

use crate::bindings::{Bindings, Control, InputDevice};
use crate::pad::PadInput;
use bevy::ecs::system::SystemParam;
use crate::control::{BodyView, PlayerControl, Pov};
use crate::interface::{amber, at, u};
use crate::lockpick::screen::{Ctx, FontKind, Kit};
use crate::phys::PhysState;
use crate::photo_lights::{CameraHold, Inputs, PhotoLights, Route, Row};
use crate::player::Player;
use crate::{space, PlaySet, PlayState, PlayerCamera};
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use fnv_phys::Filter;
use nvf_assets::GameData;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

// ---- the camera [design] ---------------------------------------------------------------

/// How far the point the camera circles may move from the player's pivot (game units,
/// about 3 m): a short range, so photo mode cannot scout.
pub const MAX_PAN: f32 = 200.0;
/// The camera's distance from that point: nearest, farthest, and at the start when the
/// view was first person.
pub const MIN_DIST: f32 = 30.0;
pub const MAX_DIST: f32 = 320.0;
const START_DIST: f32 = 150.0;
/// The camera keeps this far from what it hits.
const WALL_MARGIN: f32 = 10.0;
/// Radians per pixel of mouse motion, per second at a full stick.
const LOOK_PER_PIXEL: f32 = 0.003;
const STICK_TURN: f32 = 2.2;
/// Units per second the camera moves (Shift: three times).
const MOVE_SPEED: f32 = 140.0;
const MAX_PITCH: f32 = 1.45;
/// The free camera flies anywhere up to this far from the player (game units, about 57 m
/// by the Focus text's scale): well inside the cells loaded round the player, so the
/// view never reaches the edge of the loaded world [design].
pub const MAX_FREE_RANGE: f32 = 4000.0;
/// Units per second the free camera flies (Shift: three times).
const FLY_SPEED: f32 = 320.0;
/// Depth of field: the band kept sharp round the focus and the ramp to full blur past
/// it, as parts of the focus distance plus a floor (game units) [design].
const DOF_SHARP: (f32, f32) = (0.06, 12.0);
const DOF_RAMP: (f32, f32) = (0.6, 80.0);

// ---- the panel -------------------------------------------------------------------------

/// The panel: its width, its right edge from the view's, its top (interface units).
const PANEL_W: f32 = 430.0;
const PANEL_RIGHT: f32 = 70.0;
const PANEL_TOP: f32 = 210.0;
const PAD: f32 = 22.0;
const ROW_STEP: f32 = 62.0;
const TRACK_DROP: f32 = 40.0;
const DIM: f32 = 120.0;
/// Seconds photo mode's notice ("Photo saved") stays up.
const NOTICE_SECONDS: f32 = 2.5;

/// A held Left/Right repeats after this long, this often (seconds).
const REPEAT_AFTER: f32 = 0.3;
const REPEAT_EVERY: f32 = 0.045;

/// The panel's tabs, as photo modes commonly group their options [design].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Camera,
    Look,
    Frame,
    /// Photo lights (`crate::photo_lights`): its rows are [`crate::photo_lights::Row`].
    Lights,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Camera, Tab::Look, Tab::Frame, Tab::Lights];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Camera => "CAMERA",
            Tab::Look => "LOOK",
            Tab::Frame => "FRAME",
            Tab::Lights => "LIGHTS",
        }
    }

    /// Its options, in the panel's order.
    pub fn settings(self) -> &'static [Setting] {
        match self {
            Tab::Camera => &[Setting::Camera, Setting::Zoom, Setting::Roll, Setting::Focus, Setting::Blur],
            Tab::Look => &[Setting::Brightness, Setting::Contrast, Setting::Saturation, Setting::Warmth, Setting::Vignette, Setting::Grain],
            Tab::Frame => &[Setting::Letterbox, Setting::Grid, Setting::HidePlayer],
            Tab::Lights => &[],
        }
    }
}

/// The options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Free (flies anywhere within [`MAX_FREE_RANGE`]) or Orbit (circles the player).
    Camera,
    Zoom,
    Roll,
    /// Depth of field: the distance kept sharp ("Auto": what is under the screen's centre).
    Focus,
    /// Depth of field: how strongly what is off the focus blurs (0: off).
    Blur,
    Brightness,
    Contrast,
    Saturation,
    /// -1 cool (blue) to 1 warm (orange).
    Warmth,
    Vignette,
    Grain,
    /// Black bars to an aspect ([`LETTERBOXES`]); they are in the photo.
    Letterbox,
    /// Rule-of-thirds lines over the view; never in the photo.
    Grid,
    /// The player's body is not drawn (its shadow stays).
    HidePlayer,
}

/// The letterbox choices: name, width over height (0: none).
pub const LETTERBOXES: [(&str, f32); 6] = [("Off", 0.0), ("2.39:1", 2.39), ("1.85:1", 1.85), ("16:9", 16.0 / 9.0), ("4:3", 4.0 / 3.0), ("1:1", 1.0)];
const CAMERAS: [&str; 2] = ["Free", "Orbit"];
const OFF_ON: [&str; 2] = ["Off", "On"];
const GRIDS: [&str; 2] = ["Off", "Thirds"];

/// The Focus slider's far end in game units, reached on a curve so near distances get
/// the finer steps [design].
pub const FOCUS_FAR: f32 = 8000.0;

impl Setting {
    pub const ALL: [Setting; 14] = [
        Setting::Camera,
        Setting::Zoom,
        Setting::Roll,
        Setting::Focus,
        Setting::Blur,
        Setting::Brightness,
        Setting::Contrast,
        Setting::Saturation,
        Setting::Warmth,
        Setting::Vignette,
        Setting::Grain,
        Setting::Letterbox,
        Setting::Grid,
        Setting::HidePlayer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::Camera => "Camera",
            Setting::Zoom => "Zoom (FOV)",
            Setting::Roll => "Roll",
            Setting::Focus => "Focus",
            Setting::Blur => "Blur",
            Setting::Brightness => "Brightness",
            Setting::Contrast => "Contrast",
            Setting::Saturation => "Saturation",
            Setting::Warmth => "Warmth",
            Setting::Vignette => "Vignette",
            Setting::Grain => "Film Grain",
            Setting::Letterbox => "Letterbox",
            Setting::Grid => "Grid",
            Setting::HidePlayer => "Hide Player",
        }
    }

    /// The test aid's name for it.
    pub fn key(self) -> &'static str {
        match self {
            Setting::Camera => "camera",
            Setting::Zoom => "zoom",
            Setting::Roll => "roll",
            Setting::Focus => "focus",
            Setting::Blur => "blur",
            Setting::Brightness => "brightness",
            Setting::Contrast => "contrast",
            Setting::Saturation => "saturation",
            Setting::Warmth => "warmth",
            Setting::Vignette => "vignette",
            Setting::Grain => "grain",
            Setting::Letterbox => "letterbox",
            Setting::Grid => "grid",
            Setting::HidePlayer => "hide",
        }
    }

    pub fn by_name(name: &str) -> Option<Setting> {
        Setting::ALL.into_iter().find(|s| s.key().eq_ignore_ascii_case(name.trim()))
    }

    /// A choice's names (its value is the index), or `None` for a slider.
    pub fn choices(self) -> Option<&'static [&'static str]> {
        static LB: [&str; 6] = [LETTERBOXES[0].0, LETTERBOXES[1].0, LETTERBOXES[2].0, LETTERBOXES[3].0, LETTERBOXES[4].0, LETTERBOXES[5].0];
        match self {
            Setting::Camera => Some(&CAMERAS),
            Setting::Letterbox => Some(&LB),
            Setting::Grid => Some(&GRIDS),
            Setting::HidePlayer => Some(&OFF_ON),
            _ => None,
        }
    }

    pub fn tab(self) -> Tab {
        Tab::ALL.into_iter().find(|t| t.settings().contains(&self)).unwrap_or(Tab::Camera)
    }

    /// Lowest, highest, one step.
    pub fn range(self) -> (f32, f32, f32) {
        if let Some(c) = self.choices() {
            return (0.0, (c.len() - 1) as f32, 1.0);
        }
        match self {
            // The game's field of view in degrees (across 4:3).
            Setting::Zoom => (10.0, 120.0, 2.0),
            Setting::Roll => (-45.0, 45.0, 1.0),
            // The place along the Focus curve, 0 Auto ([`focus_units`]).
            Setting::Focus => (0.0, 100.0, 1.0),
            Setting::Blur | Setting::Vignette | Setting::Grain => (0.0, 1.0, 0.05),
            Setting::Brightness | Setting::Contrast => (0.5, 1.5, 0.02),
            Setting::Saturation => (0.0, 2.0, 0.05),
            Setting::Warmth => (-1.0, 1.0, 0.05),
            _ => (0.0, 1.0, 1.0),
        }
    }

    /// The default value.
    pub fn default_value(self) -> f32 {
        match self {
            Setting::Brightness | Setting::Contrast | Setting::Saturation => 1.0,
            _ => 0.0,
        }
    }
}

/// The Focus slider's place (1 to 100) as a distance in game units; 0 is Auto.
pub fn focus_units(place: f32) -> Option<f32> {
    (place >= 0.5).then(|| (place / 100.0).powi(2) * FOCUS_FAR)
}

/// The options' values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhotoSettings {
    /// The field of view, the game's degrees; the view's own at the start.
    pub fov: f32,
    /// The rest, by [`Setting::ALL`]'s order (the zoom's place unused).
    values: [f32; 14],
    /// The field of view photo mode opened with (Reset goes back to it).
    pub start_fov: f32,
}

impl PhotoSettings {
    /// Every option at its default, the zoom at `fov` (the view's).
    pub fn new(fov: f32) -> PhotoSettings {
        let (lo, hi, _) = Setting::Zoom.range();
        let fov = fov.clamp(lo, hi);
        PhotoSettings { fov, values: Setting::ALL.map(Setting::default_value), start_fov: fov }
    }

    fn index(s: Setting) -> usize {
        Setting::ALL.iter().position(|&x| x == s).unwrap_or(0)
    }

    pub fn get(&self, s: Setting) -> f32 {
        if s == Setting::Zoom {
            self.fov
        } else {
            self.values[Self::index(s)]
        }
    }

    /// Set an option, kept in its range (a choice to a whole index).
    pub fn set(&mut self, s: Setting, v: f32) {
        let (lo, hi, _) = s.range();
        let v = if v.is_finite() { v.clamp(lo, hi) } else { self.get(s) };
        let v = if s.choices().is_some() { v.round() } else { v };
        if s == Setting::Zoom {
            self.fov = v;
        } else {
            self.values[Self::index(s)] = v;
        }
    }

    /// Move an option `steps` steps (negative: down), rounded to its steps; a choice goes
    /// round from its last to its first.
    pub fn step(&mut self, s: Setting, steps: f32) {
        let (lo, hi, step) = s.range();
        if s.choices().is_some() {
            let n = hi - lo + 1.0;
            self.set(s, (self.get(s) + steps.signum()).rem_euclid(n));
            return;
        }
        let v = ((self.get(s) + step * steps) / step).round() * step;
        self.set(s, v);
    }

    /// Back to the defaults (the zoom to where it opened).
    pub fn reset(&mut self) {
        *self = PhotoSettings::new(self.start_fov);
    }

    /// The slider's place along its track, 0 to 1.
    pub fn share(&self, s: Setting) -> f32 {
        let (lo, hi, _) = s.range();
        ((self.get(s) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// The value as the panel writes it.
    pub fn text(&self, s: Setting) -> String {
        if let Some(c) = s.choices() {
            return c.get(self.get(s) as usize).copied().unwrap_or("?").to_string();
        }
        let signed = |v: i32| match v {
            0 => "0".to_string(),
            r => format!("{r:+}"),
        };
        match s {
            // The HUD font has no degree sign [observed: the panel's first screenshots].
            Setting::Zoom => format!("{:.0}", self.fov),
            Setting::Roll => signed(self.get(s).round() as i32),
            Setting::Warmth => signed((self.get(s) * 100.0).round() as i32),
            Setting::Focus => match focus_units(self.get(s)) {
                None => "Auto".into(),
                Some(u) => {
                    let m = u / crate::pipboy::data::model::UNITS_PER_METRE;
                    if m < 10.0 {
                        format!("{m:.1} m")
                    } else {
                        format!("{m:.0} m")
                    }
                }
            },
            Setting::Blur | Setting::Vignette | Setting::Grain if self.get(s) == 0.0 => "Off".into(),
            _ => format!("{:.0}%", self.get(s) * 100.0),
        }
    }

    pub fn free_camera(&self) -> bool {
        self.get(Setting::Camera) == 0.0
    }

    pub fn roll(&self) -> f32 {
        self.get(Setting::Roll)
    }

    pub fn grid(&self) -> bool {
        self.get(Setting::Grid) != 0.0
    }

    pub fn hide_player(&self) -> bool {
        self.get(Setting::HidePlayer) != 0.0
    }

    /// Width over height of the letterbox, 0 for none.
    pub fn letterbox(&self) -> f32 {
        LETTERBOXES.get(self.get(Setting::Letterbox) as usize).map_or(0.0, |l| l.1)
    }
}

impl Default for PhotoSettings {
    fn default() -> Self {
        PhotoSettings::new(75.0)
    }
}

/// The game's field of view (degrees across 4:3) a vertical angle in radians stands for:
/// the inverse of `crate::free_cam::vertical_fov`.
pub fn game_fov(vertical: f32) -> f32 {
    (2.0 * ((vertical / 2.0).tan() * 4.0 / 3.0).atan()).to_degrees()
}

// ---- the file ----------------------------------------------------------------------

/// `<home>\Photos`, beside the saves and the logs.
pub fn photos_dir(home: &Path) -> PathBuf {
    home.join("Photos")
}

/// A place's name as a part of a file name: no characters Windows forbids, spaces kept
/// single, at most 60 characters; "Unknown" for none.
pub fn file_safe_place(place: &str) -> String {
    let cleaned: String = place.chars().map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let mut out = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > 60 {
        out = out.chars().take(60).collect::<String>().trim_end().to_string();
    }
    // Windows does not end a name with a dot.
    let out = out.trim_end_matches('.').trim().to_string();
    if out.is_empty() {
        "Unknown".into()
    } else {
        out
    }
}

/// The photo's file name for a local date and time (year, month, day, hour, minute,
/// second) and a place: "NVForever 2026-11-10 01-23-45 Goodsprings.png".
pub fn photo_file_name(when: (i32, u32, u32, u32, u32, u32), place: &str) -> String {
    let (y, mo, d, h, mi, s) = when;
    format!("NVForever {y:04}-{mo:02}-{d:02} {h:02}-{mi:02}-{s:02} {}.png", file_safe_place(place))
}

/// `name` in `dir`, or with " (2)", " (3)" … before the extension when `taken` says a file
/// of that name is there (two photos in one second).
pub fn unique_path(dir: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !taken(&first) {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, "png"));
    (2..10_000).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !taken(p)).unwrap_or(first)
}

/// The captured window as tight RGB rows (alpha dropped), from its texture format.
pub fn rgb_rows(data: &[u8], format: bevy::render::render_resource::TextureFormat) -> Result<Vec<u8>, String> {
    use bevy::render::render_resource::TextureFormat as F;
    let bgra = match format {
        F::Rgba8Unorm | F::Rgba8UnormSrgb => false,
        F::Bgra8Unorm | F::Bgra8UnormSrgb => true,
        other => return Err(format!("the window's format {other:?} is not one photo mode writes")),
    };
    if !data.len().is_multiple_of(4) {
        return Err("the capture is not whole pixels".into());
    }
    Ok(data.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| if bgra { [b, g, r] } else { [r, g, b] }).collect())
}

fn write_png(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(rgb).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())
}

// ---- the state -----------------------------------------------------------------------

/// Where a photo is in its making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shot {
    #[default]
    Idle,
    /// The interface is hidden this frame; the capture is asked for on the next.
    Hiding,
    /// Asked for; waiting for the picture.
    Capturing,
}

/// Photo mode.
#[derive(Resource, Default)]
pub struct PhotoMode {
    /// Open.
    pub on: bool,
    /// Open and taking the controls this frame (no console over it).
    pub active: bool,
    pub settings: PhotoSettings,
    /// The tab shown ([`Tab::ALL`]) and the option chosen on it.
    pub tab: usize,
    pub selected: usize,
    pub panel_hidden: bool,
    /// The camera: the compass heading and pitch it looks along (radians), its distance
    /// from the point it circles, and that point's offset from the player's pivot (game
    /// space).
    heading: f32,
    pitch: f32,
    dist: f32,
    pan: [f32; 3],
    /// The free camera's place (game space); `None` until it starts where the orbit put it.
    free_at: Option<[f32; 3]>,
    /// The camera's place this frame, and the distance to what is under the screen's
    /// centre (game units), for the Focus slider's Auto.
    cam_at: [f32; 3],
    auto_focus: f32,
    /// The view the player had, and the camera's vertical field of view under photo
    /// mode's zoom: what the game's own writers (the settings, the sights' and a
    /// conversation's zoom) have on it, followed while open and put back on close.
    pov: Pov,
    vfov: f32,
    /// The zoom photo mode last put on the camera, taken off again by [`lift`] before
    /// the game's writers read it (they would take it for the unzoomed view; [observed: buckallobill, in game] the view
    /// stayed narrow after photo mode).
    applied: Option<f32>,
    shot: Shot,
    /// Photo mode's own notice ("Photo saved") and the real seconds it has left: drawn
    /// by photo mode, with the panel shown or hidden, as the corner messages wait while
    /// it is open.
    pub notice: Option<(String, f32)>,
    /// The interface roots photo mode hid, and their visibility before.
    hidden: HashMap<Entity, Visibility>,
    /// Left/Right held: the direction and for how long.
    held: Option<(f32, f32)>,
    repeat_at: f32,
    /// The photo lights and the Lights tab's chosen row (`crate::photo_lights::Row::ALL`).
    pub lights: PhotoLights,
    pub light_row: usize,
}

impl PhotoMode {
    /// Photo mode holds the world as a menu does.
    pub fn holds_world(&self) -> bool {
        self.on
    }

    /// Brightness, saturation and contrast factors for the grade while open.
    pub fn look(&self) -> Option<(f32, f32, f32)> {
        let s = &self.settings;
        self.on.then(|| (s.get(Setting::Brightness), s.get(Setting::Saturation), s.get(Setting::Contrast)))
    }

    /// The grade's photo look: warmth, vignette, film grain, letterbox aspect (all 0 when
    /// closed).
    pub fn finish(&self) -> [f32; 4] {
        if !self.on {
            return [0.0; 4];
        }
        let s = &self.settings;
        [s.get(Setting::Warmth), s.get(Setting::Vignette), s.get(Setting::Grain), s.letterbox()]
    }

    /// The depth of field while open with some blur: strength, the band kept sharp, the
    /// distance at which the blur is full, and the focus (game units along the view).
    pub fn depth_of_field(&self) -> Option<PhotoDof> {
        let s = &self.settings;
        let strength = s.get(Setting::Blur);
        if !self.on || strength <= 0.0 {
            return None;
        }
        let focus = focus_units(s.get(Setting::Focus)).unwrap_or(self.auto_focus).max(1.0);
        let sharp = focus * DOF_SHARP.0 + DOF_SHARP.1;
        Some(PhotoDof { strength, sharp, full: sharp + focus * DOF_RAMP.0 + DOF_RAMP.1, focus })
    }

    /// The player's body is hidden for the picture.
    pub fn hides_player(&self) -> bool {
        self.on && self.settings.hide_player()
    }

    fn capturing(&self) -> bool {
        self.shot != Shot::Idle
    }

    fn tab(&self) -> Tab {
        Tab::ALL[self.tab.min(Tab::ALL.len() - 1)]
    }

    fn setting(&self) -> Option<Setting> {
        let rows = self.tab().settings();
        rows.get(self.selected.min(rows.len().saturating_sub(1))).copied()
    }

    fn light_row(&self) -> Row {
        Row::ALL[self.light_row.min(Row::ALL.len() - 1)]
    }

    /// The Lights tab is shown.
    pub fn lights_tab(&self) -> bool {
        self.on && self.tab() == Tab::Lights
    }

    /// The light markers are drawn: the Lights tab shown with the panel, or a light being
    /// edited; never while a photo is taken.
    pub fn markers_shown(&self) -> bool {
        self.on && !self.capturing() && (self.lights.editing.is_some() || (self.lights_tab() && !self.panel_hidden))
    }

    /// The camera's place (game space) and its heading and pitch (radians), this frame.
    pub fn camera_view(&self) -> ([f32; 3], f32, f32) {
        (self.cam_at, self.heading, self.pitch)
    }

    fn camera_hold(&self) -> CameraHold {
        CameraHold { heading: self.heading, pitch: self.pitch, dist: self.dist, pan: self.pan, free_at: self.free_at }
    }

    /// Stop editing a light and put the camera back exactly where it stood.
    fn end_light_edit(&mut self) {
        if let Some(h) = self.lights.end_edit() {
            (self.heading, self.pitch, self.dist, self.pan, self.free_at) = (h.heading, h.pitch, h.dist, h.pan, h.free_at);
            info!("photo mode: light editing ends; the camera is back at ({:.1}, {:.1}, {:.1}) heading {:.4} pitch {:.4}", self.cam_at[0], self.cam_at[1], self.cam_at[2], h.heading, h.pitch);
        }
    }

    fn begin_light_edit(&mut self) {
        let hold = self.camera_hold();
        self.lights.begin_edit(hold);
        if self.lights.editing.is_some() {
            info!("photo mode: editing light {}; the camera stands at ({:.1}, {:.1}, {:.1}) heading {:.4} pitch {:.4}", self.lights.current + 1, self.cam_at[0], self.cam_at[1], self.cam_at[2], hold.heading, hold.pitch);
        }
    }

    fn add_light(&mut self) {
        let hold = self.camera_hold();
        if self.lights.add(self.cam_at, hold) {
            self.light_row = Row::ALL.iter().position(|&r| r == Row::Edit).unwrap_or(0);
            info!("photo mode: light {} added at the camera ({:.0}, {:.0}, {:.0}); editing it, the camera stands still", self.lights.lights.len(), self.cam_at[0], self.cam_at[1], self.cam_at[2]);
            info!("photo mode: editing light {}; the camera stands at ({:.1}, {:.1}, {:.1}) heading {:.4} pitch {:.4}", self.lights.current + 1, self.cam_at[0], self.cam_at[1], self.cam_at[2], hold.heading, hold.pitch);
        } else {
            info!("photo mode: no more lights (at most {})", crate::photo_lights::MAX_LIGHTS);
        }
    }
}

/// Photo mode's depth of field (`PhotoMode::depth_of_field`; imagespace.rs draws it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhotoDof {
    pub strength: f32,
    /// Within this of the focus nothing blurs; the blur is full this far from it.
    pub sharp: f32,
    pub full: f32,
    pub focus: f32,
}

/// Asked to open photo mode on the next frame in the world (the pause menu's entry).
#[derive(Resource, Default)]
pub struct PhotoRequest(pub bool);

/// Photo mode's keys and buttons: fixed, but for its opener, the Photo Mode binding
/// ([`Control::PhotoMode`]: unbound by default, #3634) [design].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoKey {
    /// Open or close photo mode: the Photo Mode binding ([`Control::PhotoMode`]).
    Open,
    /// Tab (Shift+Tab is the previous tab), RB.
    NextTab,
    /// LB.
    PreviousTab,
    /// Up, Down: the panel's rows (the D-pad's).
    RowUp,
    RowDown,
    /// Right, Left: the row's value (the D-pad's).
    More,
    Less,
    /// H, Y: hide or show the panel.
    Hide,
    /// R, X: the options back to their defaults.
    Reset,
    /// Enter (the keypad's too) or the left button, A: take the photo.
    Take,
}

impl PhotoKey {
    fn keys(self) -> &'static [KeyCode] {
        use KeyCode::*;
        match self {
            PhotoKey::Open => &[],
            PhotoKey::NextTab => &[Tab],
            PhotoKey::PreviousTab => &[],
            PhotoKey::RowUp => &[ArrowUp],
            PhotoKey::RowDown => &[ArrowDown],
            PhotoKey::More => &[ArrowRight],
            PhotoKey::Less => &[ArrowLeft],
            PhotoKey::Hide => &[KeyH],
            PhotoKey::Reset => &[KeyR],
            PhotoKey::Take => &[Enter, NumpadEnter],
        }
    }

    fn mouse(self) -> Option<MouseButton> {
        (self == PhotoKey::Take).then_some(MouseButton::Left)
    }

    fn pad(self) -> Option<GamepadButton> {
        use GamepadButton::*;
        Some(match self {
            PhotoKey::Open => return None,
            PhotoKey::NextTab => RightTrigger,
            PhotoKey::PreviousTab => LeftTrigger,
            PhotoKey::RowUp => DPadUp,
            PhotoKey::RowDown => DPadDown,
            PhotoKey::More => DPadRight,
            PhotoKey::Less => DPadLeft,
            PhotoKey::Hide => North,
            PhotoKey::Reset => West,
            PhotoKey::Take => South,
        })
    }
}

/// Photo mode's input: its fixed keys and buttons ([`PhotoKey`]), Esc, and the Photo Mode
/// binding that opens and closes it, read by its key, mouse button or controller input.
#[derive(SystemParam)]
pub struct PhotoInput<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    pads: crate::pad::ActionPads<'w, 's>,
    bindings: Res<'w, Bindings>,
    sticks: Option<Res<'w, crate::pad::StickButtons>>,
}

impl PhotoInput<'_, '_> {
    /// `k` is down (`now`: gone down this frame).
    fn read(&self, k: PhotoKey, now: bool) -> bool {
        if k == PhotoKey::Open {
            return self.bound(Control::PhotoMode, now);
        }
        let key = k.keys().iter().any(|&x| if now { self.keys.just_pressed(x) } else { self.keys.pressed(x) });
        let mouse = k.mouse().is_some_and(|m| if now { self.mouse.just_pressed(m) } else { self.mouse.pressed(m) });
        let pad = k.pad().is_some_and(|b| self.pads.iter().any(|g| if now { g.just_pressed(b) } else { g.pressed(b) }));
        key || mouse || pad
    }

    /// Binding `c` is down (`now`: gone down this frame): its key, mouse button or
    /// controller input.
    fn bound(&self, c: Control, now: bool) -> bool {
        let b = self.bindings.get(c);
        let key = b.key.is_some_and(|k| if now { self.keys.just_pressed(k) } else { self.keys.pressed(k) });
        let mouse = b.mouse.is_some_and(|m| if now { self.mouse.just_pressed(m) } else { self.mouse.pressed(m) });
        let pad = match self.bindings.pad(c) {
            Some(PadInput::Button(x)) => self.pads.iter().any(|g| if now { g.just_pressed(x) } else { g.pressed(x) }),
            Some(PadInput::Stick(d)) => self.sticks.as_ref().is_some_and(|s| if now { s.just_pressed(d) } else { s.pressed(d) }),
            None => false,
        };
        key || mouse || pad
    }

    /// `k` is held down.
    pub fn pressed(&self, k: PhotoKey) -> bool {
        self.read(k, false)
    }

    /// `k` went down this frame.
    pub fn just_pressed(&self, k: PhotoKey) -> bool {
        self.read(k, true)
    }

    pub fn key_pressed(&self, k: KeyCode) -> bool {
        self.keys.pressed(k)
    }

    pub fn key_just_pressed(&self, k: KeyCode) -> bool {
        self.keys.just_pressed(k)
    }

    pub fn mouse_just_pressed(&self, m: MouseButton) -> bool {
        self.mouse.just_pressed(m)
    }

    pub fn pad_just_pressed(&self, b: GamepadButton) -> bool {
        self.pads.iter().any(|g| g.just_pressed(b))
    }
}

/// The captured picture, from the screenshot's observer, and the writer's answers.
#[derive(Resource)]
struct Darkroom {
    picture: Option<Image>,
    send: Sender<Result<PathBuf, String>>,
    done: Mutex<Receiver<Result<PathBuf, String>>>,
}

impl Default for Darkroom {
    fn default() -> Self {
        let (send, done) = channel();
        Darkroom { picture: None, send, done: Mutex::new(done) }
    }
}

/// The panel's root.
#[derive(Component)]
struct PhotoRoot;

/// The rule-of-thirds grid's root: shown with the panel hidden too, never in the photo.
#[derive(Component)]
struct GridRoot;

/// Photo mode's notice's root ("Photo saved"): shown with the panel hidden too, never in
/// the photo.
#[derive(Component)]
struct NoticeRoot;

/// `NVF_PLAY_TEST_PHOTO`.
#[derive(Resource, Default, Debug, PartialEq)]
struct PhotoAid(Vec<(f32, String)>);

fn parse_aid(v: &str) -> Result<Vec<(f32, String)>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (t, a) = item.split_once(':').ok_or_else(|| format!("{item:?}: expected <seconds>:<action>"))?;
            let t = t.trim().parse::<f32>().map_err(|_| format!("{item:?}: bad seconds"))?;
            Ok((t, a.trim().to_string()))
        })
        .collect()
}

pub struct PhotoModePlugin;

impl Plugin for PhotoModePlugin {
    fn build(&self, app: &mut App) {
        let aid = match std::env::var("NVF_PLAY_TEST_PHOTO") {
            Ok(v) => match parse_aid(&v) {
                Ok(a) => {
                    info!("test aid NVF_PLAY_TEST_PHOTO: {} actions", a.len());
                    a
                }
                Err(e) => {
                    warn!("test aid NVF_PLAY_TEST_PHOTO: {e}");
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };
        app.init_resource::<PhotoMode>()
            .init_resource::<PhotoRequest>()
            .init_resource::<Darkroom>()
            .insert_resource(PhotoAid(aid))
            .add_systems(Update, toggle.after(PlaySet::Scripts).before(PlaySet::Ui))
            .add_systems(Update, hold_world.in_set(PlaySet::Sync))
            .add_systems(Update, place.in_set(PlaySet::Physics).after(crate::control::place_camera).after(crate::free_cam::fly))
            .add_systems(Update, (adjust, count_notice, developed, draw).chain().in_set(PlaySet::Ui))
            .add_systems(Update, lift.in_set(PlaySet::Sync).before(crate::aim_zoom::zoom).before(crate::dialogue_zoom::apply))
            .add_systems(Update, zoom.in_set(PlaySet::Sync).after(crate::aim_zoom::zoom).after(crate::dialogue_zoom::apply))
            .add_systems(PostUpdate, hide_interface.before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate))
            .add_systems(Startup, make_soft);
    }
}

/// Take the aid's actions due by `now`.
fn due(aid: &mut PhotoAid, now: f32) -> Vec<String> {
    let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut aid.0).into_iter().partition(|(t, _)| *t <= now);
    aid.0 = rest;
    due.into_iter().map(|(_, a)| a).collect()
}

/// Open and close photo mode.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn toggle(
    input: PhotoInput,
    time: crate::pause::AidTime,
    mut aid: ResMut<PhotoAid>,
    mut request: ResMut<PhotoRequest>,
    mut photo: ResMut<PhotoMode>,
    mut control: ResMut<PlayerControl>,
    (state, console, paused, menu, loading, free_cam, kill_cam): (
        Res<State<PlayState>>,
        Res<crate::console::Console>,
        Res<crate::pause::Paused>,
        Res<crate::menu::ActiveMenu>,
        Res<crate::loading::Loading>,
        Option<Res<crate::free_cam::FreeCam>>,
        Option<Res<crate::kill_cam::KillCam>>,
    ),
    (player, body): (Option<Res<Player>>, Res<BodyView>),
    mut next: ResMut<NextState<PlayState>>,
    cams: Query<(&Transform, &Projection), With<PlayerCamera>>,
    mut aid_actions: Local<Vec<String>>,
) {
    // `open` and `close` are this system's; the rest go on to `adjust`.
    for a in due(&mut aid, time.elapsed_secs()) {
        if a == "open" || a == "close" {
            aid_actions.push(a);
        } else {
            aid.0.push((f32::NEG_INFINITY, a));
        }
    }
    let by_aid = |acts: &mut Vec<String>, name: &str| {
        let hit = acts.iter().position(|a| a == name);
        hit.map(|i| acts.remove(i)).is_some()
    };
    // The Photo Mode binding, read here: `ControlInput` holds every play control while
    // photo mode is open.
    let pressed = input.just_pressed(PhotoKey::Open);
    if photo.on {
        photo.active = !console.open;
        if !photo.active || photo.capturing() {
            return;
        }
        let back = input.key_just_pressed(KeyCode::Escape) || input.pad_just_pressed(GamepadButton::East);
        // Editing a light: Esc, B or the right button stop editing (the camera comes
        // back); photo mode stays open.
        if photo.lights.editing.is_some() && (back || input.mouse_just_pressed(MouseButton::Right)) {
            photo.end_light_edit();
            return;
        }
        let close = back || pressed || by_aid(&mut aid_actions, "close");
        if close {
            close_photo(&mut photo, &mut control, &mut next);
        }
        return;
    }
    // The pause menu's entry asks while the menu is still up: the request waits for play
    // to resume (`crate::graphics_resume` can hold the resume a few frames).
    let resuming = request.0 && (paused.0 || *state.get() != PlayState::Playing);
    if resuming {
        return;
    }
    let asked = std::mem::take(&mut request.0) || by_aid(&mut aid_actions, "open") || pressed;
    if !asked {
        return;
    }
    let free = *state.get() == PlayState::Playing && !console.open && !paused.0 && menu.0.is_none() && !loading.active() && !free_cam.is_some_and(|f| f.on) && !kill_cam.is_some_and(|k| k.active());
    let (Some(p), Ok((t, proj))) = (player, cams.single()) else { return };
    if !free {
        info!("photo mode: not now (a menu, the console, a loading screen or a camera of its own has the view)");
        return;
    }
    let vfov = match proj {
        Projection::Perspective(p) => p.fov,
        _ => crate::free_cam::vertical_fov(75.0),
    };
    let pivot = body.pivot.unwrap_or(p.eye());
    let cam = space::to_game(t.translation);
    let v = [pivot[0] - cam[0], pivot[1] - cam[1], pivot[2] - cam[2]];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (heading, pitch, dist) = if len < 20.0 {
        // From first person: behind the player, looking a little down.
        (control.view_heading(p.heading), (p.pitch - 0.25).clamp(-MAX_PITCH, MAX_PITCH), START_DIST)
    } else {
        (v[0].atan2(v[1]), (v[2] / len).asin().clamp(-MAX_PITCH, MAX_PITCH), len.clamp(MIN_DIST, MAX_DIST))
    };
    let f = &mut *photo;
    (f.on, f.active, f.selected, f.panel_hidden, f.shot, f.held, f.notice) = (true, true, 0, false, Shot::Idle, None, None);
    (f.heading, f.pitch, f.dist, f.pan, f.pov, f.vfov) = (heading, pitch, dist, [0.0; 3], control.pov, vfov);
    (f.tab, f.free_at, f.auto_focus, f.light_row) = (0, None, dist, 0);
    f.lights.clear();
    f.settings = PhotoSettings::new(game_fov(vfov));
    control.pov = Pov::Third;
    next.set(PlayState::Menu);
    info!("photo mode: open (the game pauses), zoom {:.0}°", f.settings.fov);
}

fn close_photo(photo: &mut PhotoMode, control: &mut PlayerControl, next: &mut NextState<PlayState>) {
    photo.on = false;
    photo.active = false;
    control.pov = photo.pov;
    next.set(PlayState::Playing);
    photo.settings.reset();
    if !photo.lights.lights.is_empty() {
        info!("photo mode: {} photo lights removed", photo.lights.lights.len());
    }
    photo.lights.clear();
    photo.light_row = 0;
    info!("photo mode: closed, the view is back on the player");
}

/// The world stays held while photo mode is open (as the free camera's mode 1).
fn hold_world(photo: Res<PhotoMode>, console: Res<crate::console::Console>, paused: Res<crate::pause::Paused>, menu: Res<crate::menu::ActiveMenu>, mut next: ResMut<NextState<PlayState>>, state: Res<State<PlayState>>) {
    if photo.holds_world() && !console.open && !paused.0 && menu.0.is_none() && matches!(*state.get(), PlayState::Playing | PlayState::Menu) {
        NextState::set_if_different(&mut *next, PlayState::Menu);
    }
}

/// The left and right triggers' pull, 0 to 1.
fn trigger(pads: &crate::pad::ActionPads, b: GamepadButton) -> f32 {
    pads.iter().filter_map(|g| g.get(b)).fold(0.0, f32::max)
}

/// Move the camera and put the view there; after `place_camera`, so it wins.
#[allow(clippy::too_many_arguments)]
fn place(
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    (motion, scroll): (Res<AccumulatedMouseMotion>, Res<AccumulatedMouseScroll>),
    pads: crate::pad::ActionPads,
    player: Option<Res<Player>>,
    body: Res<BodyView>,
    mut phys: ResMut<PhysState>,
    mut photo: ResMut<PhotoMode>,
    mut cams: Query<&mut Transform, With<PlayerCamera>>,
    time: crate::pause::AidTime,
    mut aid: Local<Option<Vec<(f32, String)>>>,
) {
    // The aid's camera moves (orbit, near), on the aids' clock.
    let aid = aid.get_or_insert_with(|| {
        std::env::var("NVF_PLAY_TEST_PHOTO").ok().and_then(|v| parse_aid(&v).ok()).unwrap_or_default().into_iter().filter(|(_, a)| a.starts_with("orbit=") || a.starts_with("near=") || a.starts_with("fly=") || a.starts_with("lift=") || a.starts_with("light-turn=") || a.starts_with("light-move=") || a == "face").collect()
    });
    if !photo.on {
        return;
    }
    let (Some(p), Ok(mut t)) = (player, cams.single_mut()) else { return };
    let dt = real.delta_secs().min(0.1);
    let f = &mut *photo;
    let now = time.elapsed_secs();
    let (mut fly, mut lift, mut face) = (0.0f32, 0.0f32, false);
    // The aid's light moves: `light-turn=<yaw>:<pitch>` (degrees), `light-move=<ahead>:<side>:<up>`
    // (game units), on the light being edited, as the camera's inputs would.
    let mut aid_light: Option<Inputs> = None;
    aid.retain(|(at, a)| {
        if *at > now {
            return true;
        }
        let nums = |r: &str| r.split(':').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>();
        if let Some(v) = a.strip_prefix("light-turn=").map(nums) {
            let i = aid_light.get_or_insert_with(Inputs::default);
            i.look += Vec2::new(v.first().copied().unwrap_or(0.0).to_radians(), v.get(1).copied().unwrap_or(0.0).to_radians());
            return false;
        }
        if let Some(v) = a.strip_prefix("light-move=").map(nums) {
            let i = aid_light.get_or_insert_with(Inputs::default);
            (i.ahead, i.side, i.up) = (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0));
            return false;
        }
        true
    });
    aid.retain(|(at, a)| {
        if *at > now {
            return true;
        }
        if let Some(d) = a.strip_prefix("orbit=").and_then(|d| d.parse::<f32>().ok()) {
            f.heading += d.to_radians();
            info!("test aid NVF_PLAY_TEST_PHOTO: orbit {d}°");
        } else if let Some(d) = a.strip_prefix("near=").and_then(|d| d.parse::<f32>().ok()) {
            f.dist = d.clamp(MIN_DIST, MAX_DIST);
            info!("test aid NVF_PLAY_TEST_PHOTO: distance {:.0}", f.dist);
        } else if let Some(d) = a.strip_prefix("fly=").and_then(|d| d.parse::<f32>().ok()) {
            fly += d;
        } else if let Some(d) = a.strip_prefix("lift=").and_then(|d| d.parse::<f32>().ok()) {
            lift += d;
        } else if a == "face" {
            face = true;
        }
        false
    });
    let free = f.settings.free_camera();
    let pivot = body.pivot.unwrap_or(p.eye());
    // Walls stop the camera; actors and the player's own capsule do not (`no_actors`: on
    // the ship line the rays meet the characters' capsules, which pulled the orbit camera
    // in to the back of the player's head [observed 2026-11-13, ship frame run]).
    let filter = Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), no_actors: true, ..Default::default() };
    let mut reach = |from: [f32; 3], to: [f32; 3]| -> [f32; 3] {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len <= 1e-3 {
            return from;
        }
        let r = match phys.ray(from, d, len + WALL_MARGIN, filter.clone()) {
            Some(h) => (h.distance - WALL_MARGIN).max(0.0).min(len),
            None => len,
        };
        [from[0] + d[0] / len * r, from[1] + d[1] / len * r, from[2] + d[2] / len * r]
    };
    if let (Some(i), Some(e)) = (aid_light, f.lights.editing) {
        if let Some(l) = f.lights.lights.get_mut(e) {
            crate::photo_lights::drive(l, i, f.heading, 1.0);
            info!("test aid NVF_PLAY_TEST_PHOTO: light {} now at ({:.0}, {:.0}, {:.0}) direction {:+.0} tilt {:+.0}", e + 1, l.at[0], l.at[1], l.at[2], l.yaw, l.pitch);
        }
    }
    // The camera's inputs: to the camera, or to the light being edited (the camera then
    // stands still: `crate::photo_lights::route`).
    let mut camera_inputs = None;
    if f.active && !f.capturing() {
        let stick_r = crate::pad::stick(&pads, false);
        let stick_l = crate::pad::stick(&pads, true);
        let look = Vec2::new(motion.delta.x * LOOK_PER_PIXEL + stick_r.x * STICK_TURN * dt, -motion.delta.y * LOOK_PER_PIXEL + stick_r.y * STICK_TURN * dt);
        let k = |c: KeyCode| if keys.pressed(c) { 1.0f32 } else { 0.0 };
        let fast = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { 3.0 } else { 1.0 };
        let ahead = k(KeyCode::KeyW) - k(KeyCode::KeyS) + stick_l.y;
        let side = k(KeyCode::KeyD) - k(KeyCode::KeyA) + stick_l.x;
        let up = k(KeyCode::Space) + k(KeyCode::KeyE) - k(KeyCode::ControlLeft) - k(KeyCode::KeyQ) + trigger(&pads, GamepadButton::RightTrigger2) - trigger(&pads, GamepadButton::LeftTrigger2);
        // Width while editing: the wheel (up narrows, as it zooms in), [ and ], LB and RB.
        let pad_now = |b: GamepadButton| pads.iter().any(|g| g.just_pressed(b));
        let notch = if scroll.delta.y > 0.0 { -1.0 } else if scroll.delta.y < 0.0 { 1.0 } else { 0.0 };
        let widen = notch + k(KeyCode::BracketRight) * 30.0 * dt - k(KeyCode::BracketLeft) * 30.0 * dt + if pad_now(GamepadButton::RightTrigger) { 1.0 } else { 0.0 } - if pad_now(GamepadButton::LeftTrigger) { 1.0 } else { 0.0 };
        match crate::photo_lights::route(f.lights.editing, f.lights.lights.len(), Inputs { look, ahead, side, up, widen }) {
            Route::Light { index, inputs } => {
                let step = crate::photo_lights::MOVE_SPEED * fast * dt;
                if let Some(l) = f.lights.lights.get_mut(index) {
                    crate::photo_lights::drive(l, inputs, f.heading, step);
                }
            }
            Route::Camera(i) => camera_inputs = Some((i, fast)),
        }
    }
    if let Some((i, fast)) = camera_inputs {
        let (ahead, side, up) = (i.ahead, i.side, i.up);
        f.heading += i.look.x;
        f.pitch = (f.pitch + i.look.y).clamp(-MAX_PITCH, MAX_PITCH);
        let (sh, ch) = f.heading.sin_cos();
        let right = [ch, -sh, 0.0];
        if free {
            if let Some(at) = f.free_at {
                // Along the view (its pitch too), sideways, and straight up and down; one
                // axis at a time, so the camera slides along a wall it meets.
                let step = FLY_SPEED * fast * dt;
                let dir = space::look_dir(f.heading, f.pitch);
                let mv = [0, 1, 2].map(|i| (dir[i] * ahead + right[i] * side) * step + if i == 2 { up * step } else { 0.0 });
                let mut now = at;
                for i in 0..3 {
                    let mut to = now;
                    to[i] += mv[i];
                    now = reach(now, to);
                }
                // Kept within reach of the player.
                let off = [now[0] - pivot[0], now[1] - pivot[1], now[2] - pivot[2]];
                let len = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
                if len > MAX_FREE_RANGE {
                    now = [0, 1, 2].map(|i| pivot[i] + off[i] * MAX_FREE_RANGE / len);
                }
                f.free_at = Some(now);
            }
        } else {
            let step = MOVE_SPEED * fast * dt;
            // Nearer and farther.
            f.dist = (f.dist - ahead * step).clamp(MIN_DIST, MAX_DIST);
            // Sideways, up and down: the point circled moves.
            for (p, r) in f.pan.iter_mut().zip(right) {
                *p += r * side * step;
            }
            f.pan[2] += up * step;
            let len = (f.pan[0] * f.pan[0] + f.pan[1] * f.pan[1] + f.pan[2] * f.pan[2]).sqrt();
            if len > MAX_PAN {
                f.pan = f.pan.map(|x| x * MAX_PAN / len);
            }
        }
    }
    // The aid's flight: straight up, then along the view, walls stopping it; `face` turns
    // the free camera to the player.
    if f.lights.editing.is_some() && (fly != 0.0 || lift != 0.0 || face) {
        info!("test aid NVF_PLAY_TEST_PHOTO: the camera stands still while a light is edited");
        (fly, lift, face) = (0.0, 0.0, false);
    }
    if let Some(at) = f.free_at.filter(|_| free && (fly != 0.0 || lift != 0.0)) {
        let dir = space::look_dir(f.heading, f.pitch);
        let up = reach(at, [at[0], at[1], at[2] + lift]);
        let to = reach(up, [0, 1, 2].map(|i| up[i] + dir[i] * fly));
        let gone = [0, 1, 2].map(|i| to[i] - pivot[i]);
        info!("test aid NVF_PLAY_TEST_PHOTO: the free camera flies {fly:.0}, now {:.0} from the player", (gone[0] * gone[0] + gone[1] * gone[1] + gone[2] * gone[2]).sqrt());
        f.free_at = Some(to);
    }
    if let Some(at) = f.free_at.filter(|_| free && face) {
        let v = [pivot[0] - at[0], pivot[1] - at[1], pivot[2] - at[2]];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
        (f.heading, f.pitch) = (v[0].atan2(v[1]), (v[2] / len).asin().clamp(-MAX_PITCH, MAX_PITCH));
        info!("test aid NVF_PLAY_TEST_PHOTO: the free camera faces the player from {len:.0}");
    }
    let out = match f.free_at.filter(|_| free) {
        Some(at) => at,
        None => {
            // The orbit: from the pivot to the point circled, then back along the view.
            let focus = [pivot[0] + f.pan[0], pivot[1] + f.pan[1], pivot[2] + f.pan[2]];
            let dir = space::look_dir(f.heading, f.pitch);
            let want = [focus[0] - dir[0] * f.dist, focus[1] - dir[1] * f.dist, focus[2] - dir[2] * f.dist];
            let at_focus = reach(pivot, focus);
            let out = if at_focus == focus { reach(focus, want) } else { at_focus };
            // The free camera starts where the orbit stands.
            if free {
                f.free_at = Some(out);
            } else {
                f.free_at = None;
            }
            out
        }
    };
    // What is under the screen's centre, for the Focus slider's Auto (anything solid,
    // actors too).
    let dir = space::look_dir(f.heading, f.pitch);
    f.auto_focus = phys.ray(out, dir, FOCUS_FAR, Filter::default()).map_or(FOCUS_FAR, |h| h.distance);
    f.cam_at = out;
    // Pinned lights ride the camera.
    for l in f.lights.lights.iter_mut().filter(|l| l.pinned) {
        l.at = out;
    }
    t.translation = space::to_bevy(out);
    t.rotation = space::look_rotation(f.heading, f.pitch) * Quat::from_rotation_z(-f.settings.roll().to_radians());
}

/// Take photo mode's zoom off the camera: what lies under it goes back before the game's
/// writers of the field of view read it (`crate::aim_zoom::zoom`,
/// `crate::dialogue_zoom::apply`, which take whatever they did not write for the unzoomed
/// view). Something else that wrote since (the settings, the console) is the view under
/// now. After a close it stays there: photo mode never saves its zoom anywhere.
fn lift(mut photo: ResMut<PhotoMode>, mut cams: Query<&mut Projection, With<PlayerCamera>>) {
    let Some(applied) = photo.applied.take() else { return };
    let Ok(mut proj) = cams.single_mut() else { return };
    let Projection::Perspective(p) = &mut *proj else { return };
    if p.fov == applied {
        p.fov = photo.vfov;
    } else {
        photo.vfov = p.fov;
    }
    if !photo.on {
        info!("photo mode: the field of view is back at vertical {:.1}°", photo.vfov.to_degrees());
    }
}

/// The zoom while open, laid over what the game's writers left on the camera this frame
/// (that is kept as the view under it, [`lift`] puts it back).
fn zoom(mut photo: ResMut<PhotoMode>, mut cams: Query<&mut Projection, With<PlayerCamera>>) {
    if !photo.on {
        return;
    }
    let Ok(mut proj) = cams.single_mut() else { return };
    let Projection::Perspective(p) = &mut *proj else { return };
    let want = crate::free_cam::vertical_fov(photo.settings.fov);
    photo.vfov = p.fov;
    p.fov = want;
    photo.applied = Some(want);
}

/// A press on a Lights tab row: Add drops a light at the camera and edits it, Move & Aim
/// edits the current light (again: stops), Remove takes it away; Pin to Camera pins or
/// frees it where it points; the rest step their value.
fn light_row_press(f: &mut PhotoMode, row: Row, dir: f32) {
    match row {
        Row::Add => f.add_light(),
        Row::Edit => {
            if f.lights.editing.is_some() {
                f.end_light_edit();
            } else {
                f.begin_light_edit();
            }
        }
        Row::Remove => {
            f.end_light_edit();
            let before = f.lights.lights.len();
            f.lights.remove();
            if f.lights.lights.len() < before {
                info!("photo mode: light removed, {} left", f.lights.lights.len());
            }
        }
        Row::Pin => {
            let (cam, h, p) = f.camera_view();
            if let Some(l) = f.lights.current_mut() {
                let pin = !l.pinned;
                l.set_pinned(pin, cam, h, p);
            }
        }
        _ => f.lights.step(row, dir),
    }
}

/// The panel's keys and buttons, and the shutter.
#[allow(clippy::too_many_arguments)]
fn adjust(
    real: Res<Time<Real>>,
    keys: PhotoInput,
    scroll: Res<AccumulatedMouseScroll>,
    time: crate::pause::AidTime,
    mut aid: ResMut<PhotoAid>,
    mut photo: ResMut<PhotoMode>,
    mut commands: Commands,
) {
    // `toggle` passed these on (it runs first and keeps `open` and `close`).
    let acts = due(&mut aid, time.elapsed_secs());
    for a in acts.iter().filter_map(|a| a.strip_prefix("screen=")) {
        info!("test aid NVF_PLAY_TEST_PHOTO: screenshot {a}");
        commands.spawn(Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(PathBuf::from(a)));
    }
    if !photo.on || !photo.active {
        return;
    }
    if photo.capturing() {
        if photo.shot == Shot::Hiding {
            photo.shot = Shot::Capturing;
            commands.spawn(Screenshot::primary_window()).observe(|ev: On<ScreenshotCaptured>, mut room: ResMut<Darkroom>| room.picture = Some(ev.image.clone()));
        }
        return;
    }
    let f = &mut *photo;
    // Next Tab and Previous Tab (Tab, Shift+Tab; the shoulders) change the tab.
    let shift = keys.key_pressed(KeyCode::ShiftLeft) || keys.key_pressed(KeyCode::ShiftRight);
    let tabs = Tab::ALL.len();
    // While a light is edited the tab stays on Lights (LB/RB widen the light then).
    let editing = f.lights.editing.is_some();
    let next_tab = keys.just_pressed(PhotoKey::NextTab);
    if !editing && next_tab && !shift {
        (f.tab, f.selected) = ((f.tab + 1) % tabs, 0);
    } else if !editing && ((next_tab && shift) || keys.just_pressed(PhotoKey::PreviousTab)) {
        (f.tab, f.selected) = ((f.tab + tabs - 1) % tabs, 0);
    }
    let lights_tab = f.tab() == Tab::Lights;
    let n = if lights_tab { Row::ALL.len() } else { f.tab().settings().len() };
    let row_at = if lights_tab { &mut f.light_row } else { &mut f.selected };
    *row_at = (*row_at).min(n - 1);
    if keys.just_pressed(PhotoKey::RowDown) {
        *row_at = (*row_at + 1) % n;
    } else if keys.just_pressed(PhotoKey::RowUp) {
        *row_at = (*row_at + n - 1) % n;
    }
    // Left/Right: a step, then repeating while held.
    let dir = if keys.pressed(PhotoKey::More) {
        1.0
    } else if keys.pressed(PhotoKey::Less) {
        -1.0
    } else {
        0.0
    };
    let dt = real.delta_secs();
    let setting = f.setting();
    let row = f.light_row();
    // A choice changes once a press; a slider repeats while held.
    let choice = if lights_tab { row.slider().is_none() } else { setting.is_some_and(|s| s.choices().is_some()) };
    let mut press: Option<f32> = None;
    if dir == 0.0 {
        f.held = None;
    } else {
        match f.held {
            Some((d, since)) if d == dir => {
                let since = since + dt;
                f.held = Some((d, since));
                if since >= REPEAT_AFTER && !choice {
                    f.repeat_at += dt;
                    while f.repeat_at >= REPEAT_EVERY {
                        f.repeat_at -= REPEAT_EVERY;
                        if lights_tab {
                            f.lights.step(row, dir);
                        } else if let Some(setting) = setting {
                            f.settings.step(setting, dir);
                        }
                    }
                }
            }
            _ => {
                f.held = Some((dir, 0.0));
                f.repeat_at = 0.0;
                press = Some(dir);
            }
        }
    }
    // The wheel zooms: a notch up narrows the view (zooms in).
    let zoom_steps = if scroll.delta.y > 0.0 {
        -1.0
    } else if scroll.delta.y < 0.0 {
        1.0
    } else {
        0.0
    };
    if zoom_steps != 0.0 && !editing {
        f.settings.step(Setting::Zoom, zoom_steps);
    }
    // One press of Left/Right on a row: a value steps; an action acts.
    if let Some(dir) = press {
        if !lights_tab {
            if let Some(setting) = setting {
                f.settings.step(setting, dir);
            }
        } else {
            light_row_press(f, row, dir);
        }
    }
    for a in &acts {
        if let Some(rest) = a.strip_prefix("set=") {
            if let Some((s, v)) = rest.split_once('=').and_then(|(s, v)| Some((Setting::by_name(s)?, v.trim().parse::<f32>().ok()?))) {
                f.settings.set(s, v);
                info!("test aid NVF_PLAY_TEST_PHOTO: {} {}", s.label(), f.settings.text(s));
            }
        } else if let Some(rest) = a.strip_prefix("light=") {
            // `light=<row>=<value>` sets the current light's slider; `light=<row>` presses
            // the row (an action, or a choice's Right).
            match rest.split_once('=') {
                Some((r, v)) => {
                    if let (Some(r), Ok(v)) = (Row::by_name(r), v.trim().parse::<f32>()) {
                        f.lights.set(r, v);
                        info!("test aid NVF_PLAY_TEST_PHOTO: light {} {}", r.label(), f.lights.text(r));
                    }
                }
                None => {
                    if let Some(r) = Row::by_name(rest) {
                        light_row_press(f, r, 1.0);
                        info!("test aid NVF_PLAY_TEST_PHOTO: light {} pressed: {}", r.label(), f.lights.text(r));
                    }
                }
            }
        } else if a == "light-done" {
            f.end_light_edit();
        } else if let Some(i) = a.strip_prefix("select=").and_then(|i| i.parse::<usize>().ok()) {
            f.selected = i.min(f.tab().settings().len().saturating_sub(1));
        } else if let Some(i) = a.strip_prefix("tab=").and_then(|i| i.parse::<usize>().ok()) {
            (f.tab, f.selected) = (i.min(tabs - 1), 0);
        }
    }
    let act = |name: &str| acts.iter().any(|a| a == name);
    if keys.just_pressed(PhotoKey::Hide) || act("hide") || (act("show") && f.panel_hidden) {
        f.panel_hidden = !f.panel_hidden;
        info!("photo mode: panel {}", if f.panel_hidden { "hidden" } else { "shown" });
    }
    if keys.just_pressed(PhotoKey::Reset) && !lights_tab {
        f.settings.reset();
        info!("photo mode: sliders reset");
    }
    if keys.just_pressed(PhotoKey::Take) || act("shot") {
        f.shot = Shot::Hiding;
        info!("photo mode: taking a photo");
    }
}

/// Photo mode's notice counts down on the real clock (the game's stands still).
fn count_notice(real: Res<Time<Real>>, mut photo: ResMut<PhotoMode>) {
    let f = &mut *photo;
    if !f.on {
        f.notice = None;
        return;
    }
    if let Some((_, left)) = &mut f.notice {
        *left -= real.delta_secs();
        if *left <= 0.0 {
            f.notice = None;
        }
    }
}

/// The picture taken: name it and write it on its own thread; say when it is saved.
#[allow(clippy::too_many_arguments)]
fn developed(
    mut room: ResMut<Darkroom>,
    mut photo: ResMut<PhotoMode>,
    world: Option<Res<crate::PlayWorld>>,
    loc: Option<Res<crate::Location>>,
    player: Option<Res<Player>>,
) {
    let answers: Vec<_> = room.done.lock().map(|r| r.try_iter().collect()).unwrap_or_default();
    for a in answers {
        match a {
            Ok(path) => {
                info!("photo mode: photo saved: {}", path.display());
                info!("photo mode: notice: Photo saved");
                photo.notice = Some(("Photo saved".to_string(), NOTICE_SECONDS));
            }
            Err(e) => {
                warn!("photo mode: the photo was not saved: {e}");
                photo.notice = Some(("Photo not saved".to_string(), NOTICE_SECONDS));
            }
        }
    }
    let Some(image) = room.picture.take() else { return };
    photo.shot = Shot::Idle;
    let place = match (world.as_deref(), loc.as_deref()) {
        (Some(w), Some(l)) => crate::pipboy::data::read::place_name(w.0.lo, w.0.schema, w.0.index, l.world, l.interior, player.as_deref().map_or([0.0; 3], |p| p.feet)),
        _ => String::new(),
    };
    let send = room.send.clone();
    let Some(home) = fnv_install::EngineHome::default_root() else {
        let _ = send.send(Err("no NVForever home folder (set NVFOREVER_HOME)".into()));
        return;
    };
    // The computer's local time; UTC when its zone can't be read.
    let zone = jiff::tz::TimeZone::try_system().unwrap_or_else(|_| jiff::tz::TimeZone::unknown());
    let now = jiff::Timestamp::now().to_zoned_datetime(zone);
    let when = (i32::from(now.year()), now.month() as u32, now.day() as u32, now.hour() as u32, now.minute() as u32, now.second() as u32);
    let name = photo_file_name(when, &place);
    let dir = photos_dir(&home);
    let path = unique_path(&dir, &name, |p| p.exists());
    let (w, h) = (image.width(), image.height());
    let format = image.texture_descriptor.format;
    let data = image.data.unwrap_or_default();
    info!("photo mode: captured {w}x{h}, writing {}", path.display());
    std::thread::spawn(move || {
        let r = rgb_rows(&data, format).and_then(|rgb| write_png(&path, w, h, &rgb)).map(|_| path);
        let _ = send.send(r);
    });
}

/// While open, every piece of the interface but the panel, the grid and photo mode's
/// notice is hidden; for the picture, all of it. What was hidden comes back when photo
/// mode closes. The corner messages wait, hidden, until it closes (`crate::corner_messages`).
#[allow(clippy::type_complexity)]
fn hide_interface(
    mut photo: ResMut<PhotoMode>,
    mut roots: Query<(Entity, &mut Visibility, Has<PhotoRoot>, Has<crate::corner_messages::AreaRoot>, Has<GridRoot>, Has<NoticeRoot>), (With<Node>, Without<ChildOf>)>,
) {
    let f = &mut *photo;
    if !f.on {
        for (e, v) in f.hidden.drain() {
            if let Ok((_, mut vis, _, _, _, _)) = roots.get_mut(e) {
                *vis = v;
            }
        }
        return;
    }
    let clean = f.capturing() || f.panel_hidden;
    for (e, mut vis, panel, corner, grid, notice) in &mut roots {
        if panel {
            vis.set_if_neq(if clean { Visibility::Hidden } else { Visibility::Inherited });
            continue;
        }
        // The grid helps frame the shot with the panel hidden too, but is not in it; so
        // does the notice.
        if grid || notice {
            vis.set_if_neq(if f.capturing() { Visibility::Hidden } else { Visibility::Inherited });
            continue;
        }
        // The corner messages hide and wait themselves while photo mode is open
        // (`crate::corner_messages`).
        if corner {
            continue;
        }
        if *vis != Visibility::Hidden {
            f.hidden.entry(e).or_insert(*vis);
            *vis = Visibility::Hidden;
        }
    }
}

// ---- drawing ---------------------------------------------------------------------------

/// The soft backdrop's picture: dark, fading out over its edges (made here, no file), as
/// the casino screens' [design].
const SOFT: Handle<Image> = bevy::asset::uuid_handle!("8a4f1c2e-6b3d-4e7a-9c15-2d7e0b6f4a91");
const SOFT_SIDE: u32 = 66;
const SOFT_EDGE: f32 = 32.0;
/// As dark as the menus drawn over the world (`crate::interface::TOP_MENU_SHADE`), so the
/// amber reads over a bright sky or ground too [design; observed: buckallobill, in game: at
/// 0.8, fading in under the text, the backing was nearly gone over bright ground].
const SOFT_ALPHA: f32 = crate::interface::TOP_MENU_SHADE;
/// The fade is drawn up to this many times its picture's size, so it stays soft on a
/// large screen.
const SOFT_SCALE: f32 = 3.0;

fn soft_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let n = SOFT_SIDE;
    let ramp = |i: u32| {
        let t = ((i.min(n - 1 - i)) as f32 / SOFT_EDGE).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            px.extend_from_slice(&[0, 0, 0, (ramp(x) * ramp(y) * 255.0) as u8]);
        }
    }
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

fn make_soft(mut images: ResMut<Assets<Image>>) {
    let _ = images.insert(SOFT.id(), soft_image());
}

fn glass(p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32) {
    let slicer = TextureSlicer { border: BorderRect::all(SOFT_EDGE), center_scale_mode: SliceScaleMode::Stretch, sides_scale_mode: SliceScaleMode::Stretch, max_corner_scale: SOFT_SCALE };
    p.spawn((at(x, y, w, h), ImageNode { color: Color::srgba(1.0, 1.0, 1.0, SOFT_ALPHA), image_mode: NodeImageMode::Sliced(slicer), ..ImageNode::new(SOFT) }));
}

/// The help lines on the Lights tab: choosing rows, and while a light is edited, what
/// moves, aims and widens it.
pub fn light_help_lines(controller: bool, editing: bool) -> [&'static str; 5] {
    match (controller, editing) {
        (true, true) => ["Editing light: camera holds", "R Stick) Aim   L Stick) Move", "LT/RT) Down/Up   LB/RB) Width", "D-Pad) Options   A) Take Photo", "B) Done"],
        (true, false) => ["A) Take Photo   Y) Hide", "B) Exit   LB/RB) Tabs", "D-Pad Up/Down) Options", "D-Pad Left/Right) Change", "Add Light: then aim it"],
        (false, true) => ["Editing light: camera holds", "Mouse) Aim   WASD) Move", "Space/Ctrl) Up/Down   Wheel) Width", "Arrows) Options   Enter) Photo", "Esc or Right Click) Done"],
        (false, false) => ["Enter) Take Photo   H) Hide", "Esc) Exit   Tab) Tabs", "Up/Down) Options", "Left/Right) Change", "Add Light: then aim it"],
    }
}

/// The help lines for the device in use and the camera's mode.
pub fn help_lines(controller: bool, free: bool) -> [&'static str; 5] {
    match (controller, free) {
        (true, true) => ["A) Take Photo   Y) Hide", "X) Reset   B) Exit", "LB/RB) Tabs   D-Pad) Options", "R Stick) Look   L Stick) Fly", "LT/RT) Down/Up"],
        (true, false) => ["A) Take Photo   Y) Hide", "X) Reset   B) Exit", "LB/RB) Tabs   D-Pad) Options", "R Stick) Circle   L Stick) Move", "LT/RT) Down/Up"],
        (false, true) => ["Enter) Take Photo   H) Hide", "R) Reset   Esc) Exit", "Tab) Tabs   Arrows) Options", "Mouse) Look   WASD) Fly", "Space/Ctrl) Up/Down"],
        (false, false) => ["Enter) Take Photo   H) Hide", "R) Reset   Esc) Exit", "Tab) Tabs   Arrows) Options", "Mouse) Circle   WASD) Move", "Space/Ctrl) Up/Down"],
    }
}

fn view_key(f: &PhotoMode, controller: bool) -> Option<String> {
    f.on.then(|| {
        let s = &f.settings;
        let values: Vec<String> = Setting::ALL.iter().map(|&x| s.text(x)).collect();
        let lights: Vec<String> = Row::ALL.iter().map(|&r| f.lights.text(r)).collect();
        format!("{}:{}:{}:{controller}:{:?}:{}:{}", f.tab, f.selected, values.join("|"), f.notice.as_ref().map(|n| &n.0), f.light_row, lights.join("|"))
    })
}

/// Photo mode's own interface roots: the panel and the grid.
type OwnRoots = Or<(With<PhotoRoot>, With<GridRoot>, With<NoticeRoot>)>;

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    photo: Res<PhotoMode>,
    bindings: Res<Bindings>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    roots: Query<Entity, OwnRoots>,
    (mut kit, mut shown, glyphs): (Local<Option<Kit>>, Local<Option<String>>, Res<crate::pad::PadGlyphs>),
) {
    let controller = bindings.device == InputDevice::Controller;
    let key = view_key(&photo, controller);
    if key == *shown {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = key;
    if !photo.on {
        return;
    }
    let bc = crate::interface::bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
    let mut c = Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    spawn_panel(&mut commands, &mut c, &photo, controller);
    if photo.settings.grid() {
        spawn_grid(&mut commands);
    }
    if let Some((line, _)) = &photo.notice {
        spawn_notice(&mut commands, &mut c, line);
    }
}

/// Photo mode's notice: a line low in the middle of the view, on the panel's backing.
fn spawn_notice(commands: &mut Commands, c: &mut Ctx, line: &str) {
    const W: f32 = 360.0;
    const H: f32 = 64.0;
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() };
    let bar = Node { position_type: PositionType::Absolute, bottom: u(120.0), width: u(W), height: u(H), ..default() };
    commands.spawn((NoticeRoot, full, GlobalZIndex(60))).with_children(|r| {
        r.spawn(bar).with_children(|b| {
            glass(b, -60.0, -40.0, W + 120.0, H + 80.0);
            let x = ((W - c.text_width(FontKind::Menu, line)) / 2.0).max(0.0);
            c.text_in(b, FontKind::Menu, line, x, 14.0, false, amber(255.0));
        });
    });
}

/// Rule-of-thirds lines across the whole view, faint.
fn spawn_grid(commands: &mut Commands) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let line = Color::srgba(1.0, 1.0, 1.0, 0.35);
    commands.spawn((GridRoot, full, GlobalZIndex(55))).with_children(|g| {
        for third in [1.0 / 3.0, 2.0 / 3.0] {
            let pc = third * 100.0;
            g.spawn((Node { position_type: PositionType::Absolute, left: Val::Percent(pc), top: Val::Px(0.0), width: Val::Px(1.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(line)));
            g.spawn((Node { position_type: PositionType::Absolute, top: Val::Percent(pc), left: Val::Px(0.0), height: Val::Px(1.0), width: Val::Percent(100.0), ..default() }, BackgroundColor(line)));
        }
    });
}

fn spawn_panel(commands: &mut Commands, c: &mut Ctx, f: &PhotoMode, controller: bool) {
    let bright = amber(255.0);
    let dim = amber(DIM);
    let tab = f.tab();
    let rows = tab.settings();
    // The panel keeps the height of its longest tab, so it does not jump between tabs.
    let most = Tab::ALL.iter().map(|t| t.settings().len()).max().unwrap_or(1) as f32;
    let lights_tab = tab == Tab::Lights;
    let help = if lights_tab { light_help_lines(controller, f.lights.editing.is_some()) } else { help_lines(controller, f.settings.free_camera()) };
    let help_step = c.font_size(FontKind::Body) + 4.0;
    let title_h = 56.0;
    let tabs_h = 44.0;
    let height = PAD + title_h + tabs_h + 30.0 + most * ROW_STEP + 14.0 + help.len() as f32 * help_step + PAD;
    let panel = Node { position_type: PositionType::Absolute, right: u(PANEL_RIGHT), top: u(PANEL_TOP), width: u(PANEL_W), height: u(height), ..default() };
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((PhotoRoot, full, GlobalZIndex(60))).with_children(|r| {
        r.spawn(panel).with_children(|p| {
            // The fade (up to SOFT_EDGE x SOFT_SCALE pixels) lies outside the text.
            glass(p, -110.0, -80.0, PANEL_W + 220.0, height + 160.0);
            let mut y = PAD;
            c.text_in(p, FontKind::Title, "PHOTO MODE", PAD, y - 4.0, false, bright);
            c.text_in(p, FontKind::Body, "Paused", PAD, y + title_h - 14.0, false, dim);
            y += title_h + 30.0;
            // The tabs: the one shown bright and underlined.
            // Spread by their text's widths with even gaps, so four tabs never run together
            // [observed: at even widths, "CAMERA" met "LOOK", photo lights frames 2026-11-19].
            let widths: Vec<f32> = Tab::ALL.iter().map(|t| c.text_width(FontKind::Menu, t.label())).collect();
            let gap = ((PANEL_W - 2.0 * PAD - widths.iter().sum::<f32>()) / (Tab::ALL.len() - 1) as f32).max(8.0);
            let mut x = PAD;
            for (t, w) in Tab::ALL.into_iter().zip(widths) {
                let on = t == tab;
                c.text_in(p, FontKind::Menu, t.label(), x, y - 6.0, false, if on { bright } else { dim });
                if on {
                    c.image(p, "interface\\shared\\line\\solid.dds", at(x, y + 28.0, w, 3.0), bright);
                }
                x += w + gap;
            }
            y += tabs_h;
            c.image(p, "interface\\shared\\line\\solid.dds", at(PAD, y - 12.0, PANEL_W - 2.0 * PAD, 2.0), dim);
            let track_w = PANEL_W - 2.0 * PAD - 24.0;
            if lights_tab {
                // The Lights rows scroll: as many as the longest other tab, round the one
                // chosen.
                let shown = most as usize;
                let chosen = f.light_row.min(Row::ALL.len() - 1);
                let first = chosen.saturating_sub(shown / 2).min(Row::ALL.len() - shown);
                let none = f.lights.current().is_none();
                for (i, &r) in Row::ALL.iter().enumerate().skip(first).take(shown) {
                    let on = i == chosen;
                    // Rows of a light that is not there yet are dim.
                    let usable = !none || matches!(r, Row::Light | Row::Add);
                    let col = if on && usable { bright } else if on || usable { amber(DIM + 50.0) } else { amber(DIM - 50.0) };
                    let x = PAD + 24.0;
                    if on {
                        c.text_in(p, FontKind::Menu, ">", PAD - 2.0, y, false, bright);
                    }
                    c.text_in(p, FontKind::Menu, r.label(), x, y, false, col);
                    let text = f.lights.text(r);
                    if r.slider().is_some() && !none {
                        c.text_in(p, FontKind::Menu, &text, PANEL_W - PAD, y, true, col);
                        let ty = y + TRACK_DROP;
                        let share = f.lights.share(r);
                        c.image(p, "interface\\shared\\line\\solid.dds", at(x, ty, track_w, 2.0), dim);
                        c.image(p, "interface\\shared\\line\\solid.dds", at(x, ty - 1.0, track_w * share, 4.0), col);
                        c.image(p, "interface\\shared\\line\\solid.dds", at(x + track_w * share - 3.0, ty - 8.0, 6.0, 18.0), col);
                    } else {
                        c.text_in(p, FontKind::Menu, &format!("< {text} >"), PANEL_W - PAD, y, true, col);
                    }
                    y += ROW_STEP;
                }
                // More rows above or below: a mark in the chooser's column on the first or
                // last row shown (the chosen row sits in the middle then, so ">" is not
                // there), clear of the sliders' tracks.
                if first > 0 {
                    c.text_in(p, FontKind::Menu, "^", PAD - 2.0, y - shown as f32 * ROW_STEP, false, dim);
                }
                if first + shown < Row::ALL.len() {
                    c.text_in(p, FontKind::Menu, "v", PAD - 2.0, y - ROW_STEP, false, dim);
                }
            }
            for (i, &s) in rows.iter().enumerate() {
                let on = i == f.selected.min(rows.len() - 1);
                let col = if on { bright } else { dim };
                let x = PAD + 24.0;
                if on {
                    c.text_in(p, FontKind::Menu, ">", PAD - 2.0, y, false, bright);
                }
                c.text_in(p, FontKind::Menu, s.label(), x, y, false, col);
                if s.choices().is_some() {
                    // A choice: its value between arrows, no track.
                    c.text_in(p, FontKind::Menu, &format!("< {} >", f.settings.text(s)), PANEL_W - PAD, y, true, col);
                } else {
                    c.text_in(p, FontKind::Menu, &f.settings.text(s), PANEL_W - PAD, y, true, col);
                    let ty = y + TRACK_DROP;
                    c.image(p, "interface\\shared\\line\\solid.dds", at(x, ty, track_w, 2.0), dim);
                    let share = f.settings.share(s);
                    c.image(p, "interface\\shared\\line\\solid.dds", at(x, ty - 1.0, track_w * share, 4.0), col);
                    c.image(p, "interface\\shared\\line\\solid.dds", at(x + track_w * share - 3.0, ty - 8.0, 6.0, 18.0), col);
                }
                y += ROW_STEP;
            }
            let used = if lights_tab { most } else { rows.len() as f32 };
            y += (most - used) * ROW_STEP + 14.0;
            for line in help {
                c.text_in(p, FontKind::Body, line, PAD, y, false, dim);
                y += help_step;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field of view `vertical_fov(110)` (93.9° vertical), a wide gameplay setting.
    fn gameplay() -> f32 {
        crate::free_cam::vertical_fov(110.0)
    }

    /// The camera and the writers of its field of view in a frame's order: photo mode's
    /// lift, the sights' zoom, a conversation's zoom, photo mode's zoom.
    fn camera_app(fov: f32) -> App {
        let mut app = App::new();
        app.insert_resource(Time::<bevy::time::Real>::default())
            .insert_resource(State::new(PlayState::Playing))
            .init_resource::<PlayerControl>()
            .init_resource::<crate::combat::WeaponAnim>()
            .init_resource::<crate::aim_zoom::AimZoom>()
            .init_resource::<crate::aim_zoom::Scope>()
            .init_resource::<crate::aim_zoom::Scope3d>()
            .init_resource::<crate::dialogue_zoom::DialogueZoom>()
            .init_resource::<PhotoMode>()
            .add_systems(Update, (lift, crate::aim_zoom::zoom, crate::dialogue_zoom::apply, zoom).chain());
        app.world_mut().spawn((PlayerCamera, Projection::Perspective(PerspectiveProjection { fov, ..default() })));
        app
    }

    fn fov(app: &mut App) -> f32 {
        let mut q = app.world_mut().query_filtered::<&Projection, With<PlayerCamera>>();
        match q.single(app.world()) {
            Ok(Projection::Perspective(p)) => p.fov,
            _ => f32::NAN,
        }
    }

    fn set_fov(app: &mut App, v: f32) {
        let mut q = app.world_mut().query_filtered::<&mut Projection, With<PlayerCamera>>();
        if let Ok(mut proj) = q.single_mut(app.world_mut()) {
            if let Projection::Perspective(p) = &mut *proj {
                p.fov = v;
            }
        }
    }

    /// What `toggle` does to the view when photo mode opens.
    fn open(app: &mut App) {
        let vfov = fov(app);
        let w = app.world_mut();
        let was = w.resource::<PlayerControl>().pov;
        w.resource_mut::<PlayerControl>().pov = Pov::Third;
        let mut f = w.resource_mut::<PhotoMode>();
        (f.on, f.active, f.pov, f.vfov) = (true, true, was, vfov);
        f.settings = PhotoSettings::new(game_fov(vfov));
    }

    fn close(app: &mut App) {
        app.world_mut().resource_scope(|w, mut photo: Mut<PhotoMode>| {
            let mut next = NextState::<PlayState>::default();
            close_photo(&mut photo, &mut w.resource_mut::<PlayerControl>(), &mut next);
        });
    }

    fn photo_zoom(app: &mut App, deg: f32) {
        app.world_mut().resource_mut::<PhotoMode>().settings.set(Setting::Zoom, deg);
    }

    fn frames(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    /// A conversation's zoom eased in towards `target` (or out, none) over `n` frames.
    fn talk(app: &mut App, target: Option<f32>, n: usize) {
        for _ in 0..n {
            app.world_mut().resource_mut::<crate::dialogue_zoom::DialogueZoom>().test_step(target, 0.05);
            app.update();
        }
    }

    /// Open, zoom, close: the camera is back on the field of view it had, in the
    /// view (first person) it had, and the game's own zooms still go back to it after.
    #[test]
    fn closing_puts_back_the_field_of_view_it_opened_on() {
        let mut app = camera_app(gameplay());
        app.world_mut().resource_mut::<PlayerControl>().pov = Pov::First;
        frames(&mut app, 2);
        open(&mut app);
        photo_zoom(&mut app, 17.0);
        frames(&mut app, 5);
        assert!((fov(&mut app) - crate::free_cam::vertical_fov(17.0)).abs() < 1e-5, "photo mode's zoom is on the camera while open");
        close(&mut app);
        frames(&mut app, 3);
        assert_eq!(fov(&mut app), gameplay(), "the field of view it opened on");
        assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::First);
        let reset = app.world().resource::<PhotoMode>().settings.fov;
        assert!((reset - game_fov(gameplay())).abs() < 1e-3, "the zoom slider resets, nothing of it is kept: {reset}");
        // A conversation's zoom afterwards eases back to the gameplay view, not to photo
        // mode's.
        talk(&mut app, Some(0.4), 60);
        assert!(fov(&mut app) < gameplay() - 0.1);
        talk(&mut app, None, 60);
        assert_eq!(fov(&mut app), gameplay());
    }

    /// As played [observed: buckallobill, in game]: photo mode opened while a
    /// conversation's zoom was easing in, and the conversation ended while it was open.
    /// The zoom out went to photo mode's zoom (taken for the view under it) and the close
    /// put back the conversation's: the view stayed narrow. Both now end on the gameplay
    /// view.
    #[test]
    fn a_conversation_zoom_under_photo_mode_ends_on_the_gameplay_view() {
        let mut app = camera_app(gameplay());
        frames(&mut app, 2);
        talk(&mut app, Some(0.22), 8);
        assert!(fov(&mut app) < gameplay() - 0.1, "the conversation's zoom is coming in");
        open(&mut app);
        photo_zoom(&mut app, 40.0);
        for target in [Some(0.22), None] {
            for _ in 0..40 {
                talk(&mut app, target, 1);
                assert!((fov(&mut app) - crate::free_cam::vertical_fov(40.0)).abs() < 1e-5, "photo mode's zoom stays on top");
            }
        }
        close(&mut app);
        frames(&mut app, 3);
        assert_eq!(fov(&mut app), gameplay());
    }

    /// Down the sights when it opens: the sights' zoom holds while open, and the close
    /// gives back the zoomed view it opened on.
    #[test]
    fn the_sights_zoom_is_given_back_as_it_was() {
        let sight = crate::free_cam::vertical_fov(45.0);
        let mut app = camera_app(sight);
        app.insert_resource(crate::aim_zoom::AimZoom::down_the_sights(gameplay(), sight));
        frames(&mut app, 2);
        // The sights' zoom as it writes it (`zoomed_fov` at a blend of 1, within a rounding).
        let before = fov(&mut app);
        assert!((before - sight).abs() < 1e-5);
        open(&mut app);
        photo_zoom(&mut app, 90.0);
        frames(&mut app, 10);
        close(&mut app);
        frames(&mut app, 1);
        assert_eq!(fov(&mut app), before);
        assert_eq!(app.world().resource::<crate::aim_zoom::AimZoom>().blend, 1.0);
    }

    /// A field of view set while open (the settings, the console) is the one the close
    /// leaves.
    #[test]
    fn a_new_field_of_view_set_while_open_is_kept() {
        let mut app = camera_app(gameplay());
        frames(&mut app, 2);
        open(&mut app);
        photo_zoom(&mut app, 30.0);
        frames(&mut app, 3);
        let wider = crate::free_cam::vertical_fov(120.0);
        set_fov(&mut app, wider);
        frames(&mut app, 3);
        close(&mut app);
        frames(&mut app, 2);
        assert_eq!(fov(&mut app), wider);
    }

    #[test]
    fn the_sliders_start_at_their_defaults_and_reset_there() {
        let mut s = PhotoSettings::new(75.0);
        let g = |s: &PhotoSettings, x| s.get(x);
        assert_eq!((s.fov, g(&s, Setting::Roll), g(&s, Setting::Brightness), g(&s, Setting::Saturation), g(&s, Setting::Contrast)), (75.0, 0.0, 1.0, 1.0, 1.0));
        assert!(s.free_camera(), "the camera starts free");
        assert_eq!((g(&s, Setting::Blur), g(&s, Setting::Warmth), g(&s, Setting::Vignette), g(&s, Setting::Grain), s.letterbox()), (0.0, 0.0, 0.0, 0.0, 0.0));
        assert!(!s.grid() && !s.hide_player());
        s.set(Setting::Zoom, 40.0);
        s.set(Setting::Roll, -12.0);
        s.set(Setting::Brightness, 1.3);
        s.set(Setting::Saturation, 0.0);
        s.set(Setting::Contrast, 0.7);
        s.set(Setting::Camera, 1.0);
        s.set(Setting::Letterbox, 2.0);
        s.set(Setting::HidePlayer, 1.0);
        assert_eq!((s.fov, s.roll(), s.get(Setting::Saturation)), (40.0, -12.0, 0.0));
        assert!(!s.free_camera() && s.hide_player());
        assert_eq!(s.letterbox(), 1.85);
        s.reset();
        assert_eq!(s, PhotoSettings::new(75.0));
        assert_eq!(s.fov, 75.0, "the zoom goes back to the view's own");
    }

    #[test]
    fn the_sliders_stay_in_range_and_round_to_their_steps() {
        let mut s = PhotoSettings::new(75.0);
        s.set(Setting::Zoom, 500.0);
        assert_eq!(s.fov, 120.0);
        s.set(Setting::Roll, -90.0);
        assert_eq!(s.roll(), -45.0);
        s.set(Setting::Saturation, f32::NAN);
        assert_eq!(s.get(Setting::Saturation), 1.0, "a value that is not a number is not taken");
        s.set(Setting::Brightness, 1.0);
        for _ in 0..3 {
            s.step(Setting::Brightness, 1.0);
        }
        assert!((s.get(Setting::Brightness) - 1.06).abs() < 1e-4, "{}", s.get(Setting::Brightness));
        for _ in 0..200 {
            s.step(Setting::Brightness, -1.0);
        }
        assert_eq!(s.get(Setting::Brightness), 0.5);
        // A field of view the view opens with outside the range is brought into it.
        assert_eq!(PhotoSettings::new(150.0).fov, 120.0);
    }

    #[test]
    fn the_values_read_as_the_panel_writes_them() {
        let mut s = PhotoSettings::new(75.0);
        assert_eq!(s.text(Setting::Zoom), "75");
        assert_eq!(s.text(Setting::Roll), "0");
        s.set(Setting::Roll, 10.0);
        assert_eq!(s.text(Setting::Roll), "+10");
        s.set(Setting::Roll, -0.2);
        assert_eq!(s.text(Setting::Roll), "0");
        s.set(Setting::Roll, -7.0);
        assert_eq!(s.text(Setting::Roll), "-7");
        s.set(Setting::Saturation, 1.5);
        assert_eq!(s.text(Setting::Saturation), "150%");
        assert_eq!(s.share(Setting::Saturation), 0.75);
        assert_eq!(Setting::by_name("CONTRAST"), Some(Setting::Contrast));
        assert_eq!(Setting::by_name("zoom"), Some(Setting::Zoom));
        assert_eq!(Setting::by_name("frame"), None);
        assert_eq!(Setting::by_name("hide"), Some(Setting::HidePlayer));
        assert_eq!(s.text(Setting::Camera), "Free");
        assert_eq!(s.text(Setting::Blur), "Off");
        s.set(Setting::Blur, 0.35);
        assert_eq!(s.text(Setting::Blur), "35%");
        s.set(Setting::Warmth, -0.2);
        assert_eq!(s.text(Setting::Warmth), "-20");
        s.set(Setting::Letterbox, 1.0);
        assert_eq!(s.text(Setting::Letterbox), "2.39:1");
        assert_eq!(s.text(Setting::Focus), "Auto");
        s.set(Setting::Focus, 50.0);
        // A quarter of the far end, 2000 units, at 1.428 cm a unit.
        assert_eq!(focus_units(50.0), Some(2000.0));
        assert_eq!(s.text(Setting::Focus), "29 m");
        s.set(Setting::Focus, 10.0);
        assert_eq!(s.text(Setting::Focus), "1.1 m");
    }

    #[test]
    fn every_option_is_on_one_tab() {
        for s in Setting::ALL {
            let on: Vec<Tab> = Tab::ALL.into_iter().filter(|t| t.settings().contains(&s)).collect();
            assert_eq!(on, vec![s.tab()], "{s:?}");
        }
        assert_eq!(Tab::ALL.iter().map(|t| t.settings().len()).sum::<usize>(), Setting::ALL.len());
    }

    #[test]
    fn a_choice_goes_round_and_keeps_whole_values() {
        let mut s = PhotoSettings::new(75.0);
        s.step(Setting::Camera, 1.0);
        assert_eq!(s.text(Setting::Camera), "Orbit");
        s.step(Setting::Camera, 1.0);
        assert_eq!(s.text(Setting::Camera), "Free", "past the last choice comes the first");
        s.step(Setting::Letterbox, -1.0);
        assert_eq!(s.text(Setting::Letterbox), "1:1", "before the first comes the last");
        s.set(Setting::Grid, 0.7);
        assert_eq!(s.text(Setting::Grid), "Thirds");
        s.set(Setting::Letterbox, 99.0);
        assert_eq!(s.letterbox(), 1.0);
    }

    #[test]
    fn the_look_and_the_depth_of_field_reach_the_grade_only_while_open() {
        let mut f = PhotoMode { on: true, auto_focus: 500.0, ..Default::default() };
        f.settings.set(Setting::Warmth, 0.5);
        f.settings.set(Setting::Vignette, 0.4);
        f.settings.set(Setting::Letterbox, 3.0);
        assert_eq!(f.finish(), [0.5, 0.4, 0.0, 16.0 / 9.0]);
        assert_eq!(f.depth_of_field(), None, "no blur, no depth of field");
        f.settings.set(Setting::Blur, 0.5);
        let d = f.depth_of_field().unwrap();
        assert_eq!((d.strength, d.focus), (0.5, 500.0), "Auto focuses on what is under the centre");
        assert!(d.sharp < d.full);
        f.settings.set(Setting::Focus, 50.0);
        assert_eq!(f.depth_of_field().unwrap().focus, 2000.0);
        f.settings.set(Setting::HidePlayer, 1.0);
        assert!(f.hides_player());
        f.on = false;
        assert_eq!(f.finish(), [0.0; 4]);
        assert_eq!(f.depth_of_field(), None);
        assert!(!f.hides_player());
    }

    #[test]
    fn the_field_of_view_converts_both_ways() {
        for deg in [10.0, 55.0, 75.0, 90.0, 120.0] {
            assert!((game_fov(crate::free_cam::vertical_fov(deg)) - deg).abs() < 1e-3, "{deg}");
        }
    }

    #[test]
    fn a_photo_is_named_by_date_time_and_place() {
        assert_eq!(photo_file_name((2026, 11, 10, 1, 2, 3), "Goodsprings"), "NVForever 2026-11-10 01-02-03 Goodsprings.png");
        assert_eq!(photo_file_name((2026, 1, 9, 23, 59, 0), "The Strip: Lucky 38?"), "NVForever 2026-01-09 23-59-00 The Strip Lucky 38.png");
        assert_eq!(photo_file_name((2026, 1, 9, 0, 0, 0), ""), "NVForever 2026-01-09 00-00-00 Unknown.png");
    }

    #[test]
    fn place_names_are_made_safe_for_files() {
        assert_eq!(file_safe_place("  Vault 21\\Gift  Shop  "), "Vault 21 Gift Shop");
        assert_eq!(file_safe_place("a<b>c|d*e\"f/g"), "a b c d e f g");
        assert_eq!(file_safe_place("Dot."), "Dot");
        assert_eq!(file_safe_place("\t\n"), "Unknown");
        let long = "x".repeat(100);
        assert_eq!(file_safe_place(&long).chars().count(), 60);
    }

    #[test]
    fn two_photos_in_one_second_get_their_own_files() {
        let dir = Path::new("C:/home/Photos");
        let taken = [dir.join("a.png"), dir.join("a (2).png")];
        assert_eq!(unique_path(dir, "b.png", |p| taken.contains(&p.to_path_buf())), dir.join("b.png"));
        assert_eq!(unique_path(dir, "a.png", |p| taken.contains(&p.to_path_buf())), dir.join("a (3).png"));
        assert_eq!(photos_dir(Path::new("C:/Games/home")), Path::new("C:/Games/home").join("Photos"));
    }

    #[test]
    fn the_capture_turns_into_rgb_whatever_its_order() {
        use bevy::render::render_resource::TextureFormat as F;
        let px = [10, 20, 30, 255, 40, 50, 60, 255];
        assert_eq!(rgb_rows(&px, F::Rgba8UnormSrgb).unwrap(), vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(rgb_rows(&px, F::Bgra8UnormSrgb).unwrap(), vec![30, 20, 10, 60, 50, 40]);
        assert!(rgb_rows(&px, F::Rgba16Float).is_err());
        assert!(rgb_rows(&px[..5], F::Rgba8Unorm).is_err());
    }

    #[test]
    fn a_png_written_reads_back() {
        let dir = std::env::temp_dir().join(format!("nvf-photo-test-{}", std::process::id()));
        let path = dir.join("Photos").join("t.png");
        write_png(&path, 2, 1, &[1, 2, 3, 4, 5, 6]).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(&buf[..6], &[1, 2, 3, 4, 5, 6]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_aid_reads_its_actions() {
        assert_eq!(parse_aid("5:open, 7:set=zoom=40,9:shot").unwrap(), vec![(5.0, "open".into()), (7.0, "set=zoom=40".into()), (9.0, "shot".into())]);
        assert!(parse_aid("open").is_err());
        let mut aid = PhotoAid(parse_aid("1:a,2:b,3:c").unwrap());
        assert_eq!(due(&mut aid, 2.0), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(aid.0, vec![(3.0, "c".to_string())]);
    }

    #[test]
    fn open_holds_the_world() {
        let mut f = PhotoMode { on: true, ..Default::default() };
        assert!(f.holds_world());
        assert_eq!(f.look(), Some((1.0, 1.0, 1.0)));
        f.on = false;
        assert!(!f.holds_world());
        assert_eq!(f.look(), None);
    }

    /// Photo mode's keys are fixed but its opener [design]: Enter or the left
    /// button (A) takes the photo, and the panel's help says so; the Photo Mode binding
    /// opens it.
    #[test]
    fn photo_mode_keys_are_fixed() {
        assert!(PhotoKey::Open.keys().is_empty() && PhotoKey::Open.pad().is_none(), "the opener is the Photo Mode binding");
        assert_eq!(PhotoKey::Take.mouse(), Some(MouseButton::Left));
        assert_eq!(PhotoKey::Take.pad(), Some(GamepadButton::South));
        assert_eq!(help_lines(false, true)[0], "Enter) Take Photo   H) Hide");
        assert_eq!(help_lines(true, true)[1], "X) Reset   B) Exit");
    }
}
