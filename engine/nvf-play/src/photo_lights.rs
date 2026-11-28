//! Photo mode's lights (docs/design/photo-mode.md "Lights") [design: the mechanics of other
//! games' photo mode lights; nothing of any game's art is used]: up to [`MAX_LIGHTS`] coloured
//! lights, each set by a Hue and a Saturation slider (no colour wheel), brightness, range,
//! point or spot, a direction the player sets (never pointed at the camera on its own), a
//! width from a tight spot to a wide flood, and shadows.
//!
//! A new light drops where the camera is and goes straight into editing. While a light is
//! edited the photo camera stands still and the camera's controls move, aim and widen the
//! light instead ([`route`]); leaving editing gives the camera back exactly where it was
//! ([`CameraHold`]). "Pin to camera" is an option: a pinned light rides at the camera and its
//! direction is kept relative to the view.
//!
//! This module holds the settings and the state; nvforever draws the lights through the
//! engine's dynamic light path (`nvforever::photo_lights`) and their markers, which are
//! never in the photo. The lights live only in photo mode: they are gone when it closes,
//! and nothing of them is saved.

use bevy::prelude::*;

/// At most this many lights [design: the plan's "about 4"; each may cast shadows].
pub const MAX_LIGHTS: usize = 4;

/// The drawn photo light `i` (`nvforever::photo_lights` finds its lights by it).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhotoLightSource(pub usize);

/// Point or spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Point,
    Spot,
}

/// One light's settings. Places are game space (Z up, game units); directions a compass
/// heading and a pitch in degrees, as the camera's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhotoLight {
    pub on: bool,
    pub kind: Kind,
    /// 0-360 degrees round the colour circle (0 red, 120 green, 240 blue).
    pub hue: f32,
    /// 0 (white) to 1 (the pure hue).
    pub saturation: f32,
    /// Display units at the light, as the game's lights (`nvforever::lights::lumens`).
    pub brightness: f32,
    /// Game units the light reaches.
    pub range: f32,
    /// The spot's full cone angle, degrees: tight spot to wide flood.
    pub width: f32,
    /// Heading and pitch, degrees. Pinned: relative to the camera's view.
    pub yaw: f32,
    pub pitch: f32,
    pub shadows: bool,
    pub pinned: bool,
    pub at: [f32; 3],
}

pub const HUE: (f32, f32, f32) = (0.0, 360.0, 5.0);
pub const SATURATION: (f32, f32, f32) = (0.0, 1.0, 0.05);
pub const BRIGHTNESS: (f32, f32, f32) = (0.1, 6.0, 0.1);
pub const RANGE: (f32, f32, f32) = (100.0, 4000.0, 50.0);
pub const WIDTH: (f32, f32, f32) = (6.0, 170.0, 2.0);
pub const YAW: (f32, f32, f32) = (-180.0, 180.0, 5.0);
pub const PITCH: (f32, f32, f32) = (-89.0, 89.0, 5.0);

impl PhotoLight {
    /// A new light at `at`, aimed along the camera's view (`heading`, `pitch` in radians):
    /// warm white spot, shadows on [design].
    pub fn new(at: [f32; 3], heading: f32, pitch: f32) -> PhotoLight {
        PhotoLight {
            on: true,
            kind: Kind::Spot,
            hue: 35.0,
            saturation: 0.15,
            brightness: 2.0,
            range: 900.0,
            width: 50.0,
            yaw: wrap_deg(heading.to_degrees()),
            pitch: pitch.to_degrees().clamp(PITCH.0, PITCH.1),
            shadows: true,
            pinned: false,
            at,
        }
    }

    /// Its colour (display values 0-1): the hue at full value, mixed to white by the
    /// saturation (HSV with V = 1).
    pub fn rgb(&self) -> [f32; 3] {
        hsv_rgb(self.hue, self.saturation)
    }

    /// The direction it points in the world, given the camera's heading and pitch
    /// (radians) for a pinned light: heading and pitch in radians.
    pub fn aim(&self, cam_heading: f32, cam_pitch: f32) -> (f32, f32) {
        let (y, p) = (self.yaw.to_radians(), self.pitch.to_radians());
        if self.pinned {
            let lim = PITCH.1.to_radians();
            (cam_heading + y, (cam_pitch + p).clamp(-lim, lim))
        } else {
            (y, p)
        }
    }

    /// Pin to (or free from) the camera, keeping where it points: a pinned light's
    /// direction becomes relative to the view, a freed one absolute again, set down at
    /// the camera.
    pub fn set_pinned(&mut self, pinned: bool, cam_at: [f32; 3], cam_heading: f32, cam_pitch: f32) {
        if pinned == self.pinned {
            return;
        }
        let (h, p) = (cam_heading.to_degrees(), cam_pitch.to_degrees());
        if pinned {
            self.yaw = wrap_deg(self.yaw - h);
            self.pitch = (self.pitch - p).clamp(PITCH.0, PITCH.1);
        } else {
            self.yaw = wrap_deg(self.yaw + h);
            self.pitch = (self.pitch + p).clamp(PITCH.0, PITCH.1);
            self.at = cam_at;
        }
        self.pinned = pinned;
    }

    /// Turn it (degrees), the pitch held short of straight up and down.
    pub fn turn(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = wrap_deg(self.yaw + d_yaw);
        self.pitch = (self.pitch + d_pitch).clamp(PITCH.0, PITCH.1);
    }

    pub fn widen(&mut self, d: f32) {
        self.width = (self.width + d).clamp(WIDTH.0, WIDTH.1);
    }
}

/// Degrees into -180..180.
pub fn wrap_deg(d: f32) -> f32 {
    let w = (d + 180.0).rem_euclid(360.0) - 180.0;
    if w.is_finite() { w } else { 0.0 }
}

/// HSV with V = 1 to RGB, hue in degrees, saturation 0-1.
pub fn hsv_rgb(hue: f32, saturation: f32) -> [f32; 3] {
    let s = saturation.clamp(0.0, 1.0);
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [r, g, b].map(|c| 1.0 - s + s * c)
}

/// What the camera's inputs do this frame: drive the camera, or the light being edited.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Inputs {
    /// Look: radians from the mouse and the right stick.
    pub look: Vec2,
    /// Ahead, sideways, up (-1..1 each, times the speed factor).
    pub ahead: f32,
    pub side: f32,
    pub up: f32,
    /// Width steps: the wheel (kb) or LB/RB (pad) while editing.
    pub widen: f32,
}

/// Where the inputs go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Route {
    Camera(Inputs),
    Light { index: usize, inputs: Inputs },
}

/// Editing a light takes the camera's inputs; otherwise the camera has them.
pub fn route(editing: Option<usize>, lights: usize, inputs: Inputs) -> Route {
    match editing {
        Some(index) if index < lights => Route::Light { index, inputs },
        _ => Route::Camera(inputs),
    }
}

/// Units per second an edited light moves (fast: three times), degrees of width per
/// wheel notch.
pub const MOVE_SPEED: f32 = 240.0;
pub const WIDEN_STEP: f32 = 4.0;

/// Apply the routed inputs to the light: it turns with the look, moves along the
/// camera's frame (`cam_heading` radians: ahead and sideways level with the ground, up
/// straight up) and widens. A pinned light only turns and widens (it rides the camera).
pub fn drive(light: &mut PhotoLight, inputs: Inputs, cam_heading: f32, step: f32) {
    light.turn(inputs.look.x.to_degrees(), inputs.look.y.to_degrees());
    light.widen(inputs.widen * WIDEN_STEP);
    if light.pinned {
        return;
    }
    let (sh, ch) = cam_heading.sin_cos();
    let fwd = [sh, ch, 0.0];
    let right = [ch, -sh, 0.0];
    for i in 0..3 {
        light.at[i] += (fwd[i] * inputs.ahead + right[i] * inputs.side) * step;
    }
    light.at[2] += inputs.up * step;
}

/// The camera as it was when editing began, put back exactly when it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraHold {
    pub heading: f32,
    pub pitch: f32,
    pub dist: f32,
    pub pan: [f32; 3],
    pub free_at: Option<[f32; 3]>,
}

/// The Lights tab's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Which light the rows below set.
    Light,
    /// Add a light at the camera (and edit it).
    Add,
    /// Move and aim the light (the camera stands still).
    Edit,
    On,
    Kind,
    Hue,
    Saturation,
    Brightness,
    Range,
    Width,
    Direction,
    Tilt,
    Shadows,
    Pin,
    Remove,
}

impl Row {
    pub const ALL: [Row; 15] = [Row::Light, Row::Add, Row::Edit, Row::On, Row::Kind, Row::Hue, Row::Saturation, Row::Brightness, Row::Range, Row::Width, Row::Direction, Row::Tilt, Row::Shadows, Row::Pin, Row::Remove];

    pub fn label(self) -> &'static str {
        match self {
            Row::Light => "Light",
            Row::Add => "Add Light",
            Row::Edit => "Move & Aim",
            Row::On => "On",
            Row::Kind => "Type",
            Row::Hue => "Hue",
            Row::Saturation => "Saturation",
            Row::Brightness => "Brightness",
            Row::Range => "Range",
            Row::Width => "Width",
            Row::Direction => "Direction",
            Row::Tilt => "Tilt",
            Row::Shadows => "Shadows",
            Row::Pin => "Pin to Camera",
            Row::Remove => "Remove Light",
        }
    }

    /// A slider's range and step, `None` for a choice or an action.
    pub fn slider(self) -> Option<(f32, f32, f32)> {
        match self {
            Row::Hue => Some(HUE),
            Row::Saturation => Some(SATURATION),
            Row::Brightness => Some(BRIGHTNESS),
            Row::Range => Some(RANGE),
            Row::Width => Some(WIDTH),
            Row::Direction => Some(YAW),
            Row::Tilt => Some(PITCH),
            _ => None,
        }
    }

    /// Rows that act on a press rather than hold a value.
    pub fn action(self) -> bool {
        matches!(self, Row::Add | Row::Edit | Row::Remove)
    }

    /// The test aid's name for it.
    pub fn key(self) -> &'static str {
        match self {
            Row::Light => "light",
            Row::Add => "add",
            Row::Edit => "edit",
            Row::On => "on",
            Row::Kind => "type",
            Row::Hue => "hue",
            Row::Saturation => "sat",
            Row::Brightness => "bright",
            Row::Range => "range",
            Row::Width => "width",
            Row::Direction => "yaw",
            Row::Tilt => "pitch",
            Row::Shadows => "shadows",
            Row::Pin => "pin",
            Row::Remove => "remove",
        }
    }

    pub fn by_name(name: &str) -> Option<Row> {
        Row::ALL.into_iter().find(|r| r.key().eq_ignore_ascii_case(name.trim()))
    }
}

/// Every photo light and which one the rows set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhotoLights {
    pub lights: Vec<PhotoLight>,
    pub current: usize,
    /// The light being moved and aimed, and the camera as it stood when that began.
    pub editing: Option<usize>,
    pub hold: Option<CameraHold>,
}

impl PhotoLights {
    pub fn current(&self) -> Option<&PhotoLight> {
        self.lights.get(self.current)
    }

    pub fn current_mut(&mut self) -> Option<&mut PhotoLight> {
        self.lights.get_mut(self.current)
    }

    /// Add a light at the camera, select it and start editing it; `false` when there are
    /// already [`MAX_LIGHTS`].
    pub fn add(&mut self, at: [f32; 3], camera: CameraHold) -> bool {
        if self.lights.len() >= MAX_LIGHTS {
            return false;
        }
        self.lights.push(PhotoLight::new(at, camera.heading, camera.pitch));
        self.current = self.lights.len() - 1;
        self.begin_edit(camera);
        true
    }

    /// Start moving and aiming the current light: the camera is kept as it stands.
    pub fn begin_edit(&mut self, camera: CameraHold) {
        if self.current < self.lights.len() {
            if self.editing.is_none() {
                self.hold = Some(camera);
            }
            self.editing = Some(self.current);
        }
    }

    /// Stop editing: the camera to give back, exactly as it was.
    pub fn end_edit(&mut self) -> Option<CameraHold> {
        self.editing = None;
        self.hold.take()
    }

    /// Remove the current light (editing it ends; the camera comes back).
    pub fn remove(&mut self) -> Option<CameraHold> {
        if self.current >= self.lights.len() {
            return None;
        }
        self.lights.remove(self.current);
        self.current = self.current.min(self.lights.len().saturating_sub(1));
        self.end_edit()
    }

    /// Every light gone (photo mode closes).
    pub fn clear(&mut self) {
        *self = PhotoLights::default();
    }

    /// A row's value as the panel writes it.
    pub fn text(&self, row: Row) -> String {
        let n = self.lights.len();
        if row == Row::Add {
            return if n >= MAX_LIGHTS { format!("Max {MAX_LIGHTS}") } else { "Add".into() };
        }
        if row == Row::Light {
            return if n == 0 { "None".into() } else { format!("{} of {n}", self.current + 1) };
        }
        let Some(l) = self.current() else { return "-".into() };
        let off_on = |b: bool| if b { "On" } else { "Off" }.to_string();
        match row {
            Row::Edit => if self.editing.is_some() { "Editing".into() } else { "Edit".into() },
            Row::Remove => "Remove".into(),
            Row::On => off_on(l.on),
            Row::Kind => match l.kind {
                Kind::Point => "Point".into(),
                Kind::Spot => "Spot".into(),
            },
            Row::Hue => format!("{:.0}", l.hue),
            Row::Saturation => format!("{:.0}%", l.saturation * 100.0),
            Row::Brightness => format!("{:.0}%", l.brightness * 50.0),
            // The Focus text's scale (`UNITS_PER_METRE`, the map's).
            Row::Range => format!("{:.1} m", l.range / crate::pipboy::data::model::UNITS_PER_METRE),
            Row::Width => format!("{:.0}", l.width),
            Row::Direction => format!("{:+.0}", l.yaw),
            Row::Tilt => format!("{:+.0}", l.pitch),
            Row::Shadows => off_on(l.shadows),
            Row::Pin => off_on(l.pinned),
            Row::Add | Row::Light => unreachable!(),
        }
    }

    /// A slider's place along its track, 0-1.
    pub fn share(&self, row: Row) -> f32 {
        let (Some((lo, hi, _)), Some(l)) = (row.slider(), self.current()) else { return 0.0 };
        ((self.value(row, l) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    fn value(&self, row: Row, l: &PhotoLight) -> f32 {
        match row {
            Row::Hue => l.hue,
            Row::Saturation => l.saturation,
            Row::Brightness => l.brightness,
            Row::Range => l.range,
            Row::Width => l.width,
            Row::Direction => l.yaw,
            Row::Tilt => l.pitch,
            _ => 0.0,
        }
    }

    /// Set a slider on the current light, kept in range (the hue and the direction go
    /// round).
    pub fn set(&mut self, row: Row, v: f32) {
        let Some((lo, hi, _)) = row.slider() else { return };
        let Some(l) = self.current_mut() else { return };
        if !v.is_finite() {
            return;
        }
        match row {
            Row::Hue => l.hue = v.rem_euclid(360.0),
            Row::Direction => l.yaw = wrap_deg(v),
            _ => {
                let v = v.clamp(lo, hi);
                match row {
                    Row::Saturation => l.saturation = v,
                    Row::Brightness => l.brightness = v,
                    Row::Range => l.range = v,
                    Row::Width => l.width = v,
                    Row::Tilt => l.pitch = v,
                    _ => {}
                }
            }
        }
    }

    /// Left/Right on a value row: a slider steps, a choice flips or goes round. Actions
    /// are not values (the caller acts on them).
    pub fn step(&mut self, row: Row, dir: f32) {
        if row == Row::Light {
            let n = self.lights.len();
            if n > 0 && self.editing.is_none() {
                self.current = (self.current as i64 + dir.signum() as i64).rem_euclid(n as i64) as usize;
            }
            return;
        }
        if let Some((_, _, step)) = row.slider() {
            if let Some(l) = self.current() {
                let v = self.value(row, l);
                let v = if matches!(row, Row::Hue | Row::Direction) { v + step * dir } else { ((v + step * dir) / step).round() * step };
                self.set(row, v);
            }
            return;
        }
        let Some(l) = self.current_mut() else { return };
        match row {
            Row::On => l.on = !l.on,
            Row::Kind => l.kind = if l.kind == Kind::Point { Kind::Spot } else { Kind::Point },
            Row::Shadows => l.shadows = !l.shadows,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn hue_and_saturation_make_the_colour() {
        assert!(near(hsv_rgb(0.0, 1.0), [1.0, 0.0, 0.0]));
        assert!(near(hsv_rgb(120.0, 1.0), [0.0, 1.0, 0.0]));
        assert!(near(hsv_rgb(240.0, 1.0), [0.0, 0.0, 1.0]));
        assert!(near(hsv_rgb(60.0, 1.0), [1.0, 1.0, 0.0]));
        assert!(near(hsv_rgb(300.0, 1.0), [1.0, 0.0, 1.0]));
        assert!(near(hsv_rgb(360.0, 1.0), [1.0, 0.0, 0.0]), "the circle closes");
        // No saturation: white whatever the hue; half: halfway to white.
        assert!(near(hsv_rgb(200.0, 0.0), [1.0, 1.0, 1.0]));
        assert!(near(hsv_rgb(0.0, 0.5), [1.0, 0.5, 0.5]));
        let mut l = PhotoLight::new([0.0; 3], 0.0, 0.0);
        (l.hue, l.saturation) = (180.0, 1.0);
        assert!(near(l.rgb(), [0.0, 1.0, 1.0]));
    }

    #[test]
    fn sliders_stay_in_range_and_hue_goes_round() {
        let mut s = PhotoLights::default();
        let cam = CameraHold { heading: 0.0, pitch: 0.0, dist: 100.0, pan: [0.0; 3], free_at: None };
        assert!(s.add([0.0; 3], cam));
        s.set(Row::Hue, 355.0);
        s.step(Row::Hue, 2.0);
        assert!((s.current().unwrap().hue - 5.0).abs() < 1e-3, "hue wraps past 360");
        s.set(Row::Width, 999.0);
        assert_eq!(s.current().unwrap().width, WIDTH.1);
        s.set(Row::Width, -5.0);
        assert_eq!(s.current().unwrap().width, WIDTH.0);
        s.set(Row::Tilt, 120.0);
        assert_eq!(s.current().unwrap().pitch, PITCH.1);
        s.set(Row::Saturation, f32::NAN);
        assert!(s.current().unwrap().saturation.is_finite());
        s.step(Row::Kind, 1.0);
        assert_eq!(s.current().unwrap().kind, Kind::Point);
        assert_eq!(s.text(Row::Kind), "Point");
    }

    #[test]
    fn at_most_four_lights_and_a_new_one_drops_at_the_camera_aimed_along_the_view() {
        let mut s = PhotoLights::default();
        let cam = CameraHold { heading: 1.0, pitch: -0.3, dist: 100.0, pan: [0.0; 3], free_at: Some([5.0, 6.0, 7.0]) };
        for i in 0..MAX_LIGHTS {
            assert!(s.add([i as f32, 2.0, 3.0], cam));
            s.end_edit();
        }
        assert!(!s.add([9.0; 3], cam), "a fifth is refused");
        assert_eq!(s.lights.len(), MAX_LIGHTS);
        assert_eq!(s.text(Row::Add), "Max 4");
        let l = s.lights[0];
        assert_eq!(l.at, [0.0, 2.0, 3.0]);
        assert!((l.yaw - 1.0f32.to_degrees()).abs() < 1e-3 && (l.pitch - (-0.3f32).to_degrees()).abs() < 1e-3);
        assert!(!l.pinned, "pinning is an option, not the default");
    }

    #[test]
    fn editing_takes_the_cameras_inputs_and_gives_the_camera_back_exactly() {
        let mut s = PhotoLights::default();
        let cam = CameraHold { heading: 0.7, pitch: 0.2, dist: 140.0, pan: [1.0, 2.0, 3.0], free_at: Some([10.0, 20.0, 30.0]) };
        let inputs = Inputs { look: Vec2::new(0.1, -0.05), ahead: 1.0, side: 0.0, up: 0.0, widen: 1.0 };
        // Not editing: the camera has the inputs.
        assert_eq!(route(s.editing, s.lights.len(), inputs), Route::Camera(inputs));
        s.add([0.0; 3], cam);
        // Editing: the light has them, the camera none.
        assert_eq!(route(s.editing, s.lights.len(), inputs), Route::Light { index: 0, inputs });
        let before = s.lights[0];
        drive(&mut s.lights[0], inputs, 0.0, 10.0);
        let l = s.lights[0];
        assert!((l.yaw - (before.yaw + 0.1f32.to_degrees())).abs() < 1e-3, "the look aims the light");
        assert!((l.at[1] - 10.0).abs() < 1e-4, "ahead (heading 0 = north) moves it north");
        assert_eq!(l.width, before.width + WIDEN_STEP, "the wheel widens it");
        // Selecting another light while editing does not switch away.
        s.step(Row::Light, 1.0);
        assert_eq!(s.current, 0);
        // Editing again keeps the first hold (the camera as it stood when editing began).
        s.begin_edit(CameraHold { heading: 9.0, ..cam });
        assert_eq!(s.end_edit(), Some(cam), "the camera comes back exactly as it was");
        assert_eq!(route(s.editing, s.lights.len(), inputs), Route::Camera(inputs));
        // Removing the edited light gives the camera back too.
        s.begin_edit(cam);
        assert_eq!(s.remove(), Some(cam));
        assert!(s.lights.is_empty() && s.editing.is_none());
        assert_eq!(route(Some(0), 0, inputs), Route::Camera(inputs), "no light: the camera");
    }

    #[test]
    fn a_pinned_light_keeps_its_aim_relative_to_the_view_and_only_turns() {
        let mut l = PhotoLight::new([0.0; 3], 0.5, 0.1);
        let (h0, p0) = l.aim(0.0, 0.0);
        l.set_pinned(true, [1.0; 3], 0.5, 0.1);
        let (h, p) = l.aim(0.5, 0.1);
        assert!((h - h0).abs() < 1e-4 && (p - p0).abs() < 1e-4, "pinning keeps where it points");
        // The camera turns: so does the pinned light.
        let (h2, _) = l.aim(1.5, 0.1);
        assert!((h2 - (h0 + 1.0)).abs() < 1e-4);
        drive(&mut l, Inputs { ahead: 1.0, ..default() }, 0.0, 50.0);
        assert_eq!(l.at, [0.0; 3], "a pinned light rides the camera; the moves do not shift it");
        l.set_pinned(false, [4.0, 5.0, 6.0], 1.5, 0.1);
        let (h3, _) = l.aim(0.0, 0.0);
        assert!((wrap_deg(h3.to_degrees()) - wrap_deg((h0 + 1.0).to_degrees())).abs() < 1e-2, "freed where it pointed");
        assert_eq!(l.at, [4.0, 5.0, 6.0], "set down at the camera");
    }

    #[test]
    fn clearing_removes_every_light() {
        let mut s = PhotoLights::default();
        let cam = CameraHold { heading: 0.0, pitch: 0.0, dist: 1.0, pan: [0.0; 3], free_at: None };
        s.add([0.0; 3], cam);
        s.clear();
        assert_eq!(s, PhotoLights::default());
    }
}
