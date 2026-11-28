//! The character creator's model: the look being edited, the tab and row chosen, and
//! every change the keys, the pointer, a controller or the test aid can ask for
//! (docs/spec-notes/character-creator.md §4–§6). Drawing is `view`'s; the Bevy systems
//! are `super`'s.

use crate::control::Portrait;
use fnv_actor::creator::{self, Creator, RaceChoice, Randomise, Slider, Space, AGE_LABEL};
use fnv_actor::records::FaceGen;
use fnv_actor::Look;
use fnv_sim::FormId;
use std::sync::Arc;

/// The tabs, in order [design].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Basics,
    Face,
    Detail,
    Skin,
    Hair,
    Eyes,
}

pub const TABS: [Tab; 6] = [Tab::Basics, Tab::Face, Tab::Detail, Tab::Skin, Tab::Hair, Tab::Eyes];
/// The barber's one tab (docs/spec-notes/character-creator.md §10).
pub const BARBER_TABS: [Tab; 1] = [Tab::Hair];
/// The plastic surgeon's tabs: everything but the hair (§11 of the note).
pub const SURGEON_TABS: [Tab; 5] = [Tab::Basics, Tab::Face, Tab::Detail, Tab::Skin, Tab::Eyes];

pub use fnv_game::game::RaceMenu;

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::Basics => "BASICS",
            Tab::Face => "FACE",
            Tab::Detail => "DETAIL",
            Tab::Skin => "SKIN",
            Tab::Hair => "HAIR",
            Tab::Eyes => "EYES",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Tab::Basics => "Who you are: sex, origin, a starting face and age.",
            Tab::Face => "The shape of the face, region by region.",
            Tab::Detail => "What makes a face uneven: twists and shifts.",
            Tab::Skin => "Tone, complexion, shading and paint.",
            Tab::Hair => "Hair, its colour, beard and brows.",
            Tab::Eyes => "Eye colour and the skin around the eyes.",
        }
    }

    /// The coefficient space whose sliders the tab shows by group.
    fn space(self) -> Option<Space> {
        match self {
            Tab::Face => Some(Space::Shape),
            Tab::Detail => Some(Space::Asymmetry),
            Tab::Skin => Some(Space::Skin),
            _ => None,
        }
    }

    /// How the camera frames the player on this tab, before the player's own turning,
    /// panning and zooming ([`View`]) [design]: the whole body on Basics, the head
    /// turned a little on Hair, the face on the others.
    pub fn framing(self) -> Framing {
        match self {
            Tab::Basics => Framing { yaw: 0.0, pitch: 0.05, distance: FAR, aim: 0.0 },
            Tab::Hair => Framing { yaw: 0.45, pitch: 0.12, distance: 58.0, aim: -4.0 },
            _ => Framing { yaw: 0.15, pitch: 0.03, distance: NEAR, aim: 0.0 },
        }
    }
}

/// A tab's framing: the camera's turn and tilt, its distance, and how far above (−) or
/// below the point [`aim_drop`] gives for that distance it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub aim: f32,
}

/// How far the camera steps left, as a part of its distance from the player: negative, so
/// it steps right and the player stands in the clear part of the picture, left of the
/// panel [design: at the default field of view (75 degrees at 4:3) this size puts the
/// point looked at 275 units off the view's middle on any aspect, near the 256 that is
/// the clear part's middle; the size itself observed on the smoke run's screenshots at
/// 16:10, with the panel on the other side].
const SHIFT: f32 = -0.33;
/// The face framing's distance and the whole body's, game units [design].
pub const NEAR: f32 = 42.0;
pub const FAR: f32 = 175.0;
/// The point looked at, under the eye: the face (2) when the camera is at [`NEAR`] or
/// closer, the middle of the body (58) at [`FAR`] or further, in between in proportion
/// [design; owner 2026-10-17: zooming went "in and out on the pelvis"].
const FACE_DROP: f32 = 2.0;
const BODY_DROP: f32 = 58.0;
/// The camera's reach [design].
const MIN_DISTANCE: f32 = 20.0;
const MAX_DISTANCE: f32 = 260.0;
/// The tilt's reach (the framing's pitch plus the player's), radians: from a little
/// below the eye to well above the head.
const MIN_PITCH: f32 = -0.35;
const MAX_PITCH: f32 = 0.8;
/// How far the view may be panned: to either side, and up or down from the point
/// looked at (game units) [design: the player stays in the picture].
const PAN_SIDE: f32 = 40.0;
const PAN_UP: f32 = 30.0;
const PAN_DOWN: f32 = 80.0;

/// The point looked at, under the eye, for a camera at `distance`.
pub fn aim_drop(distance: f32) -> f32 {
    let t = ((distance - NEAR) / (FAR - NEAR)).clamp(0.0, 1.0);
    FACE_DROP + (BODY_DROP - FACE_DROP) * t
}

/// The player's own turning, tilting, panning and zooming of the creator's camera
/// (docs/spec-notes/character-creator.md §5) [owner 2026-10-17: "we need a way to drag
/// and pan our view not just zoom in and out"].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Added to the framing's turn and tilt, radians.
    pub turn: f32,
    pub tilt: f32,
    /// The framing's distance times this.
    pub zoom: f32,
    /// The picture moved right and up, game units at the point looked at.
    pub pan: [f32; 2],
}

impl Default for View {
    fn default() -> View {
        View { turn: 0.0, tilt: 0.0, zoom: 1.0, pan: [0.0, 0.0] }
    }
}

impl View {
    /// The view that frames the face on `tab`: what the creator opens with [owner
    /// 2026-10-17, todo 6: "the camera starts framed on the face instead of the whole
    /// body"]. The face tabs frame it already; on another tab the zoom brings the camera
    /// to the face framing's distance, where the point looked at is the face
    /// ([`aim_drop`]), with the face framing's tilt; the turn is the tab's own (a turn
    /// is kept from tab to tab, the player's).
    pub fn face_on(tab: Tab) -> View {
        let (f, face) = (tab.framing(), Tab::Face.framing());
        View { turn: 0.0, tilt: face.pitch - f.pitch, zoom: face.distance / f.distance, pan: [0.0, 0.0] }
    }

    /// Turn about the player (`dx`, radians; it goes all the way round) and tilt (`dy`).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.turn = (self.turn + dx + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        self.tilt += dy;
    }

    /// Move the picture right and up by game units.
    pub fn pan_by(&mut self, right: f32, up: f32) {
        self.pan[0] = (self.pan[0] + right).clamp(-PAN_SIDE, PAN_SIDE);
        self.pan[1] = (self.pan[1] + up).clamp(-PAN_UP, PAN_DOWN);
    }

    /// Zoom in (`factor` below 1) or out.
    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom *= factor;
    }

    /// The camera for a framing, everything clamped (the clamped values are kept, so a
    /// drag past a limit does not have to be undone first).
    pub fn camera(&mut self, f: Framing) -> Portrait {
        let distance = (f.distance * self.zoom).clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.zoom = distance / f.distance;
        let pitch = (f.pitch + self.tilt).clamp(MIN_PITCH, MAX_PITCH);
        self.tilt = pitch - f.pitch;
        Portrait { yaw: f.yaw + self.turn, distance, drop: aim_drop(distance) + f.aim + self.pan[1], pitch, shift: SHIFT * distance + self.pan[0] }
    }
}

/// The skin groups the Eyes tab shows instead of the Skin tab [design].
const EYE_GROUPS: [&str; 5] = ["Eyes", "Eye Sockets", "Eyelids", "Eyeliner", "Eyeshadow"];
/// The skin groups the Hair tab shows [design].
const HAIR_GROUPS: [&str; 2] = ["Beard", "Eyebrows"];

/// A row of a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Sex,
    Race,
    /// The race's starting faces (NPC_ "Is CharGen Face Preset").
    Preset,
    /// Which group of the tab's sliders is shown.
    Group,
    /// A slider, by its index in the sex's list.
    Slider(usize),
    Hair,
    /// The authored hair colours.
    HairColour,
    /// Hair colour fine-tuning: lightness, warmth, greyness (0, 1, 2).
    HairTone(u8),
    FacialHair,
    Brows,
    Eyes,
}

/// What a row asks for to be drawn: a list choice or a slider.
#[derive(Debug, Clone, PartialEq)]
pub enum RowView {
    Choice { label: String, value: String, at: usize, of: usize },
    Slider { label: String, low: String, high: String, value: String, fraction: f32, race: Option<f32> },
    Swatches { label: String, colours: Vec<[u8; 3]>, at: Option<usize> },
}

/// The buttons along the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Randomise,
    Undo,
    Reset,
    Done,
}

pub const ACTIONS: [Action; 4] = [Action::Randomise, Action::Undo, Action::Reset, Action::Done];

impl Action {
    pub fn title(self) -> &'static str {
        match self {
            Action::Randomise => "RANDOM",
            Action::Undo => "UNDO",
            Action::Reset => "RESET",
            Action::Done => "DONE",
        }
    }

    /// The key and the controller button, as the hint shows them: in the form the other
    /// menus give a button's key ("Take All [R]", `crate::pad::hint`).
    pub fn hint(self) -> &'static str {
        match self {
            Action::Randomise => "[Space] [X]",
            Action::Undo => "[Z] [Y]",
            Action::Reset => "[R] [Back]",
            Action::Done => "[Enter] [Start]",
        }
    }
}

/// The most changes Undo keeps.
const UNDO_DEPTH: usize = 64;
/// One key press moves a slider this part of its range (with Shift a quarter of it).
pub const SLIDER_STEP: f64 = 0.025;

#[derive(Debug, Clone)]
pub struct Model {
    pub data: Arc<Creator>,
    pub look: Look,
    /// The look the menu opened with (Reset).
    pub start: Look,
    undo: Vec<Look>,
    /// The row the last change was on: changes in a row run together are undone at once.
    last_edit: Option<(Tab, Row)>,
    pub tab: Tab,
    /// The chosen row of each tab.
    rows: [usize; 6],
    /// The chosen slider group of the Face, Detail, Skin, Hair and Eyes tabs.
    groups: [usize; 6],
    /// The first row shown on each tab (the list scrolls apart from the chosen row).
    scroll: [usize; 6],
    /// The starting face last chosen on the Basics tab (0 = none).
    preset: usize,
    /// The bottom buttons are chosen (Down past the last row).
    pub on_actions: Option<usize>,
    /// The player's turning, panning and zooming, over the tab's framing.
    pub view: View,
    /// Which function opened the menu: what may be changed.
    pub kind: RaceMenu,
    /// The player's tweak: a woman may have facial hair too [design: docs/design/tweaks.md].
    pub female_beards: bool,
}

fn tab_index(t: Tab) -> usize {
    TABS.iter().position(|&x| x == t).unwrap_or(0)
}

/// Cycle `i` by `by` in `0..n`.
fn cycle(i: usize, by: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    (i as i64 + i64::from(by)).rem_euclid(n as i64) as usize
}

impl Model {
    /// [design, #2403 / file refs] Logical creator state, excluding undo bookkeeping.
    pub(super) fn progress(&self) -> impl std::fmt::Debug + '_ {
        (&self.look, self.tab, self.rows, self.groups, self.scroll, self.preset, self.on_actions, &self.view)
    }
    pub fn new(data: Arc<Creator>, look: Look) -> Model {
        Model::of_kind(data, look, RaceMenu::Creator)
    }

    /// The menu as the function that opened it has it: the character creator opens on
    /// Basics framed on the face; the barber on its Hair tab, framed for the hair.
    pub fn of_kind(data: Arc<Creator>, look: Look, kind: RaceMenu) -> Model {
        let tab = if kind == RaceMenu::Barber { Tab::Hair } else { Tab::Basics };
        Model {
            data,
            start: look.clone(),
            look,
            undo: Vec::new(),
            last_edit: None,
            tab,
            rows: [0; 6],
            groups: [0; 6],
            scroll: [0; 6],
            preset: 0,
            on_actions: None,
            view: if kind == RaceMenu::Barber { View::default() } else { View::face_on(Tab::Basics) },
            kind,
            female_beards: false,
        }
    }

    /// Whether the look has no facial hair rows: a woman's, unless the tweak allows it.
    fn beardless(&self) -> bool {
        self.look.female && !self.female_beards
    }

    /// The tabs this menu shows.
    pub fn tabs(&self) -> &'static [Tab] {
        match self.kind {
            RaceMenu::Creator => &TABS,
            RaceMenu::Barber => &BARBER_TABS,
            RaceMenu::Surgeon => &SURGEON_TABS,
        }
    }

    /// The panel's title.
    pub fn title(&self) -> &'static str {
        match self.kind {
            RaceMenu::Creator => "APPEARANCE",
            RaceMenu::Barber => "BARBER",
            RaceMenu::Surgeon => "SURGEON",
        }
    }

    fn sex(&self) -> usize {
        usize::from(self.look.female)
    }

    pub fn sliders(&self) -> &[Slider] {
        &self.data.sliders[self.sex()]
    }

    pub fn race(&self) -> Option<&RaceChoice> {
        self.data.catalogue.race(self.look.race)
    }

    /// The race's FaceGen values for the sex: what the sliders read on top of the own.
    pub fn race_facegen(&self) -> Option<&FaceGen> {
        self.race().and_then(|r| r.facegen[self.sex()].as_ref())
    }

    /// The slider groups a tab cycles through, in the controls file's order.
    pub fn groups(&self, tab: Tab) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        // The surgeon has no Hair tab: the brows' shading goes with the eyes, the beard's
        // with the skin (they are the face's texture) [design].
        let surgeon = self.kind == RaceMenu::Surgeon;
        let eye_group = |g: &str| EYE_GROUPS.contains(&g) || (surgeon && g == "Eyebrows");
        let hair_group = |g: &str| !surgeon && HAIR_GROUPS.contains(&g);
        for s in self.sliders() {
            let wanted = match tab {
                Tab::Eyes => s.space() == Space::Skin && eye_group(&s.group),
                Tab::Hair => s.space() == Space::Skin && hair_group(&s.group),
                Tab::Skin => s.space() == Space::Skin && !eye_group(&s.group) && !hair_group(&s.group),
                _ => tab.space() == Some(s.space()) && s.label != AGE_LABEL,
            };
            if wanted && !out.contains(&s.group) {
                out.push(s.group.clone());
            }
        }
        // The skin's own tone first [design: what most players look for].
        if tab == Tab::Skin {
            if let Some(i) = out.iter().position(|g| g == "Skin") {
                let g = out.remove(i);
                out.insert(0, g);
            }
        }
        out
    }

    pub fn group(&self) -> Option<String> {
        let g = self.groups(self.tab);
        g.get(self.groups[tab_index(self.tab)].min(g.len().saturating_sub(1))).cloned()
    }

    /// The rows of the open tab.
    pub fn rows(&self) -> Vec<Row> {
        let sliders_of = |group: &Option<String>, space: Space| -> Vec<Row> {
            (self.sliders().iter().enumerate())
                .filter(|(_, s)| s.space() == space && Some(&s.group) == group.as_ref() && s.label != AGE_LABEL)
                .map(|(i, _)| Row::Slider(i))
                .collect()
        };
        let group = self.group();
        let age = self.sliders().iter().position(|s| s.label == AGE_LABEL);
        let mut rows = match self.tab {
            // The surgeon changes neither the sex nor the race [owner].
            Tab::Basics if self.kind == RaceMenu::Surgeon => {
                let mut r = vec![Row::Preset];
                r.extend(age.map(Row::Slider));
                r
            }
            Tab::Basics => {
                let mut r = vec![Row::Sex, Row::Race, Row::Preset];
                r.extend(age.map(Row::Slider));
                r
            }
            Tab::Face => [vec![Row::Group], sliders_of(&group, Space::Shape)].concat(),
            Tab::Detail => [vec![Row::Group], sliders_of(&group, Space::Asymmetry)].concat(),
            Tab::Skin => [vec![Row::Group], sliders_of(&group, Space::Skin)].concat(),
            Tab::Hair => {
                let mut r = vec![Row::Hair, Row::HairColour, Row::HairTone(0), Row::HairTone(1), Row::HairTone(2)];
                if !self.beardless() {
                    r.push(Row::FacialHair);
                }
                // The barber does the hair and the beard only [documented: GECK wiki
                // ShowBarberMenu, "hair color and style"; owner 2026-10-17: and beards]:
                // not the brows (part of the face) nor the face's shading.
                if self.kind == RaceMenu::Barber {
                    return r;
                }
                // Each part's shading under its row, with no Region switch: the switch sat
                // above sliders out of sight below, so it seemed to do nothing [owner
                // 2026-10-23]; the labels name the part ("Beard …", "Eyebrows …").
                let groups = self.groups(Tab::Hair);
                let shading = |g: &str| if groups.iter().any(|x| x == g) { sliders_of(&Some(g.to_string()), Space::Skin) } else { vec![] };
                // A woman has no beard [design], so no beard shading either.
                if !self.beardless() {
                    r.extend(shading("Beard"));
                }
                r.push(Row::Brows);
                r.extend(shading("Eyebrows"));
                return r;
            }
            // The surgeon's brows are here, with no Hair tab.
            Tab::Eyes if self.kind == RaceMenu::Surgeon => [vec![Row::Eyes, Row::Brows, Row::Group], sliders_of(&group, Space::Skin)].concat(),
            Tab::Eyes => [vec![Row::Eyes, Row::Group], sliders_of(&group, Space::Skin)].concat(),
        };
        if self.groups(self.tab).is_empty() {
            rows.retain(|r| *r != Row::Group);
        }
        rows
    }

    pub fn row(&self) -> usize {
        self.rows[tab_index(self.tab)].min(self.rows().len().saturating_sub(1))
    }

    /// The first row shown on the open tab.
    pub fn scroll(&self) -> usize {
        self.scroll[tab_index(self.tab)]
    }

    pub fn set_scroll(&mut self, first: usize) {
        self.scroll[tab_index(self.tab)] = first;
    }

    pub fn chosen_row(&self) -> Option<Row> {
        if self.on_actions.is_some() {
            return None;
        }
        self.rows().get(self.row()).copied()
    }

    // ---- moving about -----------------------------------------------------------------

    pub fn set_tab(&mut self, t: Tab) -> bool {
        if !self.tabs().contains(&t) {
            return false;
        }
        let changed = self.tab != t;
        self.tab = t;
        self.on_actions = None;
        // A new tab frames its own part: the zoom and pan start again, the turn stays
        // [design].
        if changed {
            self.view = View { turn: self.view.turn, ..View::default() };
        }
        changed
    }

    pub fn step_tab(&mut self, by: i32) -> bool {
        let tabs = self.tabs();
        let at = tabs.iter().position(|&x| x == self.tab).unwrap_or(0);
        self.set_tab(tabs[cycle(at, by, tabs.len())])
    }

    /// Up and Down: rows, then the bottom buttons.
    pub fn step_row(&mut self, by: i32) -> bool {
        let n = self.rows().len();
        let t = tab_index(self.tab);
        match self.on_actions {
            Some(_) if by < 0 => {
                self.on_actions = None;
                self.rows[t] = n.saturating_sub(1);
                true
            }
            Some(_) => false,
            None => {
                let r = self.row() as i64 + i64::from(by);
                if r >= n as i64 {
                    self.on_actions = Some(ACTIONS.len() - 1);
                } else {
                    self.rows[t] = r.max(0) as usize;
                }
                true
            }
        }
    }

    pub fn select_row(&mut self, i: usize) -> bool {
        let t = tab_index(self.tab);
        let changed = self.rows[t] != i || self.on_actions.is_some();
        self.rows[t] = i.min(self.rows().len().saturating_sub(1));
        self.on_actions = None;
        changed
    }

    // ---- changing the look --------------------------------------------------------------

    fn remember(&mut self, row: Row) {
        if self.last_edit != Some((self.tab, row)) {
            self.undo.push(self.look.clone());
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.last_edit = Some((self.tab, row));
    }

    /// Left and Right on the chosen row (or on the bottom buttons, which they move along).
    /// Sliders move [`SLIDER_STEP`] (`fine`: a quarter of it). Whether anything changed.
    pub fn adjust(&mut self, by: i32, fine: bool) -> bool {
        if let Some(a) = self.on_actions {
            self.on_actions = Some(cycle(a, by, ACTIONS.len()));
            return true;
        }
        let Some(row) = self.chosen_row() else { return false };
        self.adjust_row(row, by, fine)
    }

    pub fn adjust_row(&mut self, row: Row, by: i32, fine: bool) -> bool {
        let before = self.look.clone();
        let sex = self.sex();
        match row {
            Row::Group => {
                let n = self.groups(self.tab).len();
                let t = tab_index(self.tab);
                self.groups[t] = cycle(self.groups[t], by, n);
                return n > 1;
            }
            Row::Slider(i) => {
                let step = if fine { SLIDER_STEP / 4.0 } else { SLIDER_STEP };
                let f = self.slider_fraction(i);
                return self.set_slider(i, f + f64::from(by) * step);
            }
            _ => {}
        }
        self.remember(row);
        let race = self.race().cloned();
        match row {
            Row::Sex => {
                self.look.female = !self.look.female;
                self.fit_to_race();
                // A face shaped for one sex reads as that sex on the other's head
                // [observed: the record's male values on the female head, smoke run
                // screenshot]: the other sex starts from the race's first starting face.
                if let Some(p) = self.data.catalogue.presets(self.look.race, self.look.female).first() {
                    self.look.facegen = p.facegen.clone();
                    self.preset = 1;
                }
            }
            Row::Race => {
                let races = &self.data.catalogue.races;
                let i = races.iter().position(|r| r.id == self.look.race).unwrap_or(0);
                if let Some(r) = races.get(cycle(i, by, races.len())) {
                    self.look.race = r.id;
                }
                self.fit_to_race();
            }
            Row::Preset => {
                let presets = self.data.catalogue.presets(self.look.race, self.look.female);
                let n = presets.len() + 1;
                self.preset = cycle(self.preset, by, n);
                if let Some(p) = self.preset.checked_sub(1).and_then(|i| presets.get(i)) {
                    self.look.facegen = p.facegen.clone();
                    self.look.eyes = p.eyes.or(self.look.eyes);
                    if self.kind == RaceMenu::Surgeon {
                        // The face, its eyes and brows; the hair and beard stay.
                        let brows = self.data.catalogue.brows.clone();
                        let brow = p.head_parts.iter().copied().find(|h| brows.iter().any(|c| c.id == *h));
                        creator::set_part(&mut self.look, &brows, brow);
                    } else {
                        self.look.hair = p.hair.or(self.look.hair);
                        self.look.hair_color = p.hair_color.or(self.look.hair_color);
                        self.look.head_parts = p.head_parts.clone();
                    }
                } else {
                    self.look.facegen = self.start.facegen.clone();
                }
            }
            Row::Hair => {
                let hairs = race.as_ref().map(|r| r.hairs[sex].clone()).unwrap_or_default();
                // Index 0 is no hair.
                let i = self.look.hair.and_then(|h| hairs.iter().position(|c| c.id == h)).map_or(0, |i| i + 1);
                let j = cycle(i, by, hairs.len() + 1);
                self.look.hair = j.checked_sub(1).map(|j| hairs[j].id);
            }
            Row::HairColour => {
                let colours = self.swatches();
                let i = self.swatch().unwrap_or(0);
                if let Some(c) = colours.get(cycle(i, by, colours.len())) {
                    self.look.hair_color = Some(*c);
                }
            }
            Row::HairTone(k) => {
                let c = self.look.hair_color.unwrap_or([75, 50, 24]);
                let d = f32::from(by as i8) * if fine { 2.0 } else { 8.0 };
                self.look.hair_color = Some(tone(c, k, d));
            }
            Row::FacialHair | Row::Brows => {
                let kind = if row == Row::Brows { self.data.catalogue.brows.clone() } else { self.data.catalogue.facial_hair.clone() };
                let now = creator::part_of(&self.look, &kind);
                let i = now.and_then(|p| kind.iter().position(|c| c.id == p)).map_or(0, |i| i + 1);
                let j = cycle(i, by, kind.len() + 1);
                creator::set_part(&mut self.look, &kind, j.checked_sub(1).map(|j| kind[j].id));
            }
            Row::Eyes => {
                let eyes = race.as_ref().map(|r| r.eyes[sex].clone()).unwrap_or_default();
                let i = self.look.eyes.and_then(|e| eyes.iter().position(|c| c.id == e)).unwrap_or(0);
                if let Some(e) = eyes.get(cycle(i, by, eyes.len())) {
                    self.look.eyes = Some(e.id);
                }
            }
            Row::Group | Row::Slider(_) => {}
        }
        self.look != before || row == Row::Preset
    }

    /// The look as SexChange with reset leaves the player (`fnv_game::appearance`): fitted
    /// to its race and sex, then the race's first starting face for that sex, its hair,
    /// eyes and head parts included, as the Starting Face row's first entry gives it.
    pub fn reset_to_preset(&mut self) {
        self.fit_to_race();
        if let Some(p) = self.data.catalogue.presets(self.look.race, self.look.female).first() {
            self.look.facegen = p.facegen.clone();
            self.look.eyes = p.eyes.or(self.look.eyes);
            self.look.hair = p.hair.or(self.look.hair);
            self.look.hair_color = p.hair_color.or(self.look.hair_color);
            self.look.head_parts = p.head_parts.clone();
            self.preset = 1;
        }
    }

    /// The hair, eyes and beard the race and sex allow: kept when allowed, else the
    /// race's default hair and its first eyes; a woman has no beard [design].
    fn fit_to_race(&mut self) {
        let sex = self.sex();
        let Some(r) = self.race().cloned() else { return };
        if self.look.hair.is_some_and(|h| !r.hairs[sex].iter().any(|c| c.id == h)) {
            self.look.hair = r.default_hair[sex].filter(|d| r.hairs[sex].iter().any(|c| c.id == *d)).or_else(|| r.hairs[sex].first().map(|c| c.id));
        }
        if !self.look.eyes.is_some_and(|e| r.eyes[sex].iter().any(|c| c.id == e)) {
            self.look.eyes = r.eyes[sex].first().map(|c| c.id);
        }
        if self.look.female {
            let beards = self.data.catalogue.facial_hair.clone();
            creator::set_part(&mut self.look, &beards, None);
        }
        self.preset = 0;
    }

    pub fn slider_fraction(&self, i: usize) -> f64 {
        self.sliders().get(i).map_or(0.5, |s| s.fraction(&self.look.facegen, self.race_facegen()))
    }

    /// Set slider `i` to a fraction of its range.
    pub fn set_slider(&mut self, i: usize, f: f64) -> bool {
        let Some(s) = self.sliders().get(i).cloned() else { return false };
        let race = self.race_facegen().cloned();
        let before = s.fraction(&self.look.facegen, race.as_ref());
        let f = f.clamp(0.0, 1.0);
        if (f - before).abs() < 1e-9 {
            return false;
        }
        self.remember(Row::Slider(i));
        self.look.facegen = creator::full(&self.look.facegen);
        s.set_fraction(&mut self.look.facegen, race.as_ref(), f);
        true
    }

    pub fn swatches(&self) -> Vec<[u8; 3]> {
        self.data.catalogue.hair_colours.iter().take(16).map(|(c, _)| *c).collect()
    }

    /// The swatch the hair colour is, if it is one.
    pub fn swatch(&self) -> Option<usize> {
        let c = self.look.hair_color?;
        self.swatches().iter().position(|s| *s == c)
    }

    pub fn set_swatch(&mut self, i: usize) -> bool {
        let Some(c) = self.swatches().get(i).copied() else { return false };
        if self.look.hair_color == Some(c) {
            return false;
        }
        self.remember(Row::HairColour);
        self.look.hair_color = Some(c);
        true
    }

    pub fn randomise(&mut self, rng: &mut impl rand_core::RngCore) {
        self.undo.push(self.look.clone());
        self.last_edit = None;
        // The race stays: it is chosen on purpose [design].
        let before = self.look.clone();
        // The surgeon draws a face only; the barber the hair only (below).
        let what = Randomise { face: self.kind != RaceMenu::Barber, hair: self.kind != RaceMenu::Surgeon, race: false };
        self.look = creator::random_look(&self.data.catalogue, &self.look, what, rng);
        if self.kind == RaceMenu::Barber {
            // Only the hair, its colour and the beard: the eyes and brows stay.
            let beards = self.data.catalogue.facial_hair.clone();
            let beard = creator::part_of(&self.look, &beards);
            self.look.eyes = before.eyes;
            self.look.head_parts = before.head_parts.clone();
            creator::set_part(&mut self.look, &beards, beard);
        }
        if self.look.female {
            let beards = self.data.catalogue.facial_hair.clone();
            creator::set_part(&mut self.look, &beards, None);
        }
        self.preset = 0;
    }

    pub fn undo(&mut self) -> bool {
        self.last_edit = None;
        match self.undo.pop() {
            Some(l) => {
                self.look = l;
                true
            }
            None => false,
        }
    }

    pub fn reset(&mut self) -> bool {
        if self.look == self.start {
            return false;
        }
        self.undo.push(self.look.clone());
        self.last_edit = None;
        self.look = self.start.clone();
        self.preset = 0;
        true
    }

    // ---- what is drawn ------------------------------------------------------------------

    fn name_of(list: &[creator::Choice], id: Option<FormId>, none: &str) -> (String, usize) {
        match id.and_then(|i| list.iter().position(|c| c.id == i)) {
            Some(i) => (list[i].name.clone(), i + 1),
            None => (none.to_string(), 0),
        }
    }

    pub fn view(&self, row: Row) -> RowView {
        let sex = self.sex();
        let race = self.race();
        let choice = |label: &str, value: String, at: usize, of: usize| RowView::Choice { label: label.into(), value, at, of };
        match row {
            Row::Sex => choice("Sex", if self.look.female { "Female".into() } else { "Male".into() }, sex + 1, 2),
            Row::Race => {
                let races = &self.data.catalogue.races;
                let i = races.iter().position(|r| r.id == self.look.race);
                choice("Origin", race.map_or_else(|| self.look.race.to_string(), |r| r.name.clone()), i.map_or(0, |i| i + 1), races.len())
            }
            Row::Preset => {
                let n = self.data.catalogue.presets(self.look.race, self.look.female).len();
                let value = if self.preset == 0 { "Your own".to_string() } else { format!("Face {}", self.preset) };
                choice("Starting face", value, self.preset, n)
            }
            Row::Group => {
                let g = self.groups(self.tab);
                let i = self.groups[tab_index(self.tab)].min(g.len().saturating_sub(1));
                choice("Region", g.get(i).cloned().unwrap_or_default(), i + 1, g.len())
            }
            Row::Slider(i) => {
                let Some(s) = self.sliders().get(i) else { return choice("?", String::new(), 0, 0) };
                let rf = self.race_facegen();
                let f = s.fraction(&self.look.facegen, rf);
                let value = if s.label == AGE_LABEL { format!("{:.0}", s.value(&self.look.facegen, rf)) } else { format!("{:+.0}", (f * 2.0 - 1.0) * 100.0) };
                // Where the race alone puts it (its own baseline).
                let race_at = rf.map(|r| s.fraction(&creator::full(&FaceGen::default()), Some(r)) as f32);
                let label = if s.label == AGE_LABEL {
                    "Age".to_string()
                } else if s.aspect.is_empty() {
                    s.group.clone()
                } else {
                    format!("{} {}", s.group, s.aspect)
                };
                RowView::Slider { label, low: s.low.clone(), high: s.high.clone(), value, fraction: f as f32, race: race_at }
            }
            Row::Hair => {
                let hairs = race.map(|r| r.hairs[sex].clone()).unwrap_or_default();
                let (v, at) = Self::name_of(&hairs, self.look.hair, "No hair");
                choice("Hair style", v, at, hairs.len())
            }
            Row::HairColour => RowView::Swatches { label: "Hair colour".into(), colours: self.swatches(), at: self.swatch() },
            Row::HairTone(k) => {
                let c = self.look.hair_color.unwrap_or([75, 50, 24]);
                let (label, low, high) = match k {
                    0 => ("Hair shade", "Dark", "Light"),
                    1 => ("Hair warmth", "Ash", "Red"),
                    _ => ("Hair grey", "Rich", "Grey"),
                };
                let f = tone_reading(c, k);
                RowView::Slider { label: label.into(), low: low.into(), high: high.into(), value: format!("{:+.0}", (f * 2.0 - 1.0) * 100.0), fraction: f, race: None }
            }
            Row::FacialHair => {
                let (v, at) = Self::name_of(&self.data.catalogue.facial_hair, creator::part_of(&self.look, &self.data.catalogue.facial_hair), "Clean shaven");
                choice("Facial hair", v, at, self.data.catalogue.facial_hair.len())
            }
            Row::Brows => {
                let (v, at) = Self::name_of(&self.data.catalogue.brows, creator::part_of(&self.look, &self.data.catalogue.brows), "None");
                choice("Brows", v, at, self.data.catalogue.brows.len())
            }
            Row::Eyes => {
                let eyes = race.map(|r| r.eyes[sex].clone()).unwrap_or_default();
                let (v, at) = Self::name_of(&eyes, self.look.eyes, "—");
                choice("Eye colour", v, at, eyes.len())
            }
        }
    }

    /// The camera for the open tab, turned, panned and zoomed by the player.
    pub fn camera(&mut self) -> Portrait {
        let f = self.tab.framing();
        self.view.camera(f)
    }

    /// A line naming the look, for the log and the caption.
    pub fn caption(&self) -> String {
        let race = self.race().map_or_else(|| self.look.race.to_string(), |r| r.name.clone());
        format!("{} {}", if self.look.female { "Female" } else { "Male" }, race)
    }
}

/// Lightness, warmth or greyness of a colour, 0 to 1.
pub fn tone_reading(c: [u8; 3], k: u8) -> f32 {
    let [r, g, b] = c.map(f32::from);
    match k {
        0 => (r + g + b) / (3.0 * 255.0),
        1 => ((r - b) / 255.0 + 1.0) / 2.0,
        _ => {
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            1.0 - if max <= 0.0 { 0.0 } else { (max - min) / max }
        }
    }
}

/// A colour moved `d` (0–255 units) in lightness, warmth or greyness.
pub fn tone(c: [u8; 3], k: u8, d: f32) -> [u8; 3] {
    let [r, g, b] = c.map(f32::from);
    let out = match k {
        0 => [r + d, g + d, b + d],
        1 => [r + d / 2.0, g, b - d / 2.0],
        _ => {
            let m = (r + g + b) / 3.0;
            let t = (d / 64.0).clamp(-1.0, 1.0);
            // Towards the mean (grey) or away from it (rich).
            [r + (m - r) * t, g + (m - g) * t, b + (m - b) * t]
        }
    };
    out.map(|x| x.round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_actor::creator::{AuthoredFace, Catalogue, Choice, Slider};

    fn data() -> Arc<Creator> {
        let ch = |id: u32, n: &str| Choice { id: FormId(id), name: n.into() };
        let race = |id: u32, name: &str| RaceChoice {
            id: FormId(id),
            name: name.into(),
            edid: name.into(),
            hairs: [vec![ch(10, "Short"), ch(11, "Long")], vec![ch(20, "Bun")]],
            eyes: [vec![ch(30, "Blue"), ch(31, "Green")], vec![ch(30, "Blue")]],
            default_hair: [Some(FormId(10)), Some(FormId(20))],
            older: None,
            facegen: [Some(FaceGen { fggs: vec![1.0; 50], fgga: vec![0.0; 30], fgts: vec![0.0; 50] }), None],
        };
        let face = |npc: u32, v: f64| AuthoredFace {
            npc: FormId(npc),
            preset: true,
            name: None,
            race: FormId(1),
            female: false,
            facegen: FaceGen { fggs: vec![v; 50], fgga: vec![0.0; 30], fgts: vec![0.0; 50] },
            hair: Some(FormId(11)),
            hair_color: Some([200, 180, 90]),
            eyes: Some(FormId(31)),
            head_parts: vec![FormId(40)],
        };
        let unit = |m: usize, n: usize| (0..n).map(|i| if i == m { 1.0 } else { 0.0 }).collect::<Vec<f64>>();
        let slider = |label: &str, space: Space, m: usize| {
            let (group, aspect, low, high) = creator::split_label(label);
            Slider { label: label.into(), group, aspect, low, high, parts: vec![(space, unit(m, space.values()))], offset: 0.0, range: (-4.0, 4.0) }
        };
        let sliders = vec![
            Slider { offset: 30.0, range: (16.0, 70.0), ..slider(AGE_LABEL, Space::Shape, 9) },
            slider("Nose - short / long", Space::Shape, 0),
            slider("Chin - small / large", Space::Shape, 1),
            slider("Nose - tip transverse shift", Space::Asymmetry, 0),
            slider("Skin Shade Dark / Light", Space::Skin, 0),
            slider("Eyeliner Light / Dark", Space::Skin, 1),
            slider("Beard Light / Dark", Space::Skin, 2),
            slider("Eyebrows Thick / Thin", Space::Skin, 3),
        ];
        let catalogue = Catalogue {
            races: vec![race(1, "Caucasian"), race(2, "Hispanic")],
            facial_hair: vec![ch(40, "Goatee")],
            brows: vec![ch(50, "EyebrowM")],
            hair_colours: vec![([75, 50, 24], 9), ([10, 10, 10], 4)],
            faces: vec![face(100, 2.0), face(101, -2.0)],
        };
        Arc::new(Creator { catalogue, sliders: [sliders.clone(), sliders], ctl_problem: None, player: None })
    }

    fn look() -> Look {
        Look { race: FormId(1), female: false, hair: Some(FormId(10)), hair_color: Some([75, 50, 24]), eyes: Some(FormId(30)), head_parts: vec![], facegen: creator::full(&FaceGen::default()) }
    }

    #[test]
    fn tabs_hold_their_rows() {
        let mut m = Model::new(data(), look());
        assert_eq!(m.rows(), vec![Row::Sex, Row::Race, Row::Preset, Row::Slider(0)]);
        m.set_tab(Tab::Face);
        assert_eq!(m.groups(Tab::Face), vec!["Nose", "Chin"]);
        assert_eq!(m.rows(), vec![Row::Group, Row::Slider(1)]);
        assert!(m.adjust_row(Row::Group, 1, false));
        assert_eq!(m.rows(), vec![Row::Group, Row::Slider(2)]);
        m.set_tab(Tab::Skin);
        assert_eq!(m.groups(Tab::Skin), vec!["Skin"]);
        m.set_tab(Tab::Eyes);
        assert_eq!(m.rows(), vec![Row::Eyes, Row::Group, Row::Slider(5)]);
        m.set_tab(Tab::Hair);
        // Each part's shading under its row, no Region switch.
        let hair = vec![Row::Hair, Row::HairColour, Row::HairTone(0), Row::HairTone(1), Row::HairTone(2)];
        assert_eq!(m.rows(), [hair, vec![Row::FacialHair, Row::Slider(6), Row::Brows, Row::Slider(7)]].concat());
    }

    #[test]
    fn a_slider_moves_the_look_and_undo_takes_it_back() {
        let mut m = Model::new(data(), look());
        m.set_tab(Tab::Face);
        m.step_row(1);
        assert_eq!(m.chosen_row(), Some(Row::Slider(1)));
        // The race reads 1.0 on mode 0: at 5/8 of (−4, 4).
        assert!((m.slider_fraction(1) - 0.625).abs() < 1e-9);
        assert!(m.adjust(1, false) && m.adjust(1, false));
        assert!((m.slider_fraction(1) - 0.675).abs() < 1e-9);
        // Own values only: 1.0 + own = 0.675 × 8 − 4 = 1.4.
        assert!((m.look.facegen.fggs[0] - 0.4).abs() < 1e-9);
        // Both steps on one row undo together.
        assert!(m.undo());
        assert_eq!(m.look.facegen.fggs[0], 0.0);
        assert!(!m.undo());
    }

    #[test]
    fn the_tweak_offers_a_woman_facial_hair() {
        let mut m = Model::new(data(), Look { female: true, ..look() });
        m.set_tab(Tab::Hair);
        assert!(!m.rows().contains(&Row::FacialHair));
        m.female_beards = true;
        assert!(m.rows().contains(&Row::FacialHair), "bRaceMenuAllowFemaleFacialHair");
    }

    #[test]
    fn choices_cycle_and_stay_allowed() {
        let mut m = Model::new(data(), look());
        m.set_tab(Tab::Hair);
        assert!(m.adjust_row(Row::Hair, 1, false));
        assert_eq!(m.look.hair, Some(FormId(11)));
        // Past the last hair: no hair, then the first again.
        m.adjust_row(Row::Hair, 1, false);
        assert_eq!(m.look.hair, None);
        m.adjust_row(Row::Hair, 1, false);
        assert_eq!(m.look.hair, Some(FormId(10)));
        m.adjust_row(Row::FacialHair, 1, false);
        assert_eq!(m.look.head_parts, vec![FormId(40)]);
        // A woman: the race's hair for her and no beard.
        m.adjust_row(Row::Sex, 1, false);
        assert!(m.look.female);
        assert_eq!(m.look.hair, Some(FormId(20)));
        assert!(m.look.head_parts.is_empty());
        m.set_tab(Tab::Hair);
        assert!(!m.rows().contains(&Row::FacialHair) && !m.rows().contains(&Row::Slider(6)), "no beard row nor its shading");
        m.adjust_row(Row::Race, 1, false);
        assert_eq!(m.look.race, FormId(2));
    }

    #[test]
    fn a_preset_brings_its_face_and_hair() {
        let mut m = Model::new(data(), look());
        assert!(m.adjust_row(Row::Preset, 1, false));
        assert_eq!(m.look.facegen.fggs[3], 2.0);
        assert_eq!((m.look.hair, m.look.eyes), (Some(FormId(11)), Some(FormId(31))));
        assert_eq!(m.view(Row::Preset), RowView::Choice { label: "Starting face".into(), value: "Face 1".into(), at: 1, of: 2 });
        // Back to "Your own": the face the menu opened with.
        m.adjust_row(Row::Preset, -1, false);
        assert_eq!(m.look.facegen.fggs[3], 0.0);
        assert!(m.reset());
        assert_eq!(m.look, look());
    }

    #[test]
    fn randomise_keeps_the_race() {
        let mut m = Model::new(data(), look());
        let mut rng = <rand_chacha::ChaCha8Rng as rand_core::SeedableRng>::seed_from_u64(3);
        m.randomise(&mut rng);
        assert_eq!(m.look.race, FormId(1));
        // A blend of the two faces (+2, −2): strictly between.
        assert!(m.look.facegen.fggs[0].abs() < 2.0);
        assert!(m.undo());
        assert_eq!(m.look, look());
    }

    #[test]
    fn down_past_the_rows_reaches_the_buttons() {
        let mut m = Model::new(data(), look());
        for _ in 0..4 {
            m.step_row(1);
        }
        assert_eq!(m.on_actions, Some(3));
        m.adjust(-1, false);
        assert_eq!(m.on_actions, Some(2));
        m.step_row(-1);
        assert_eq!(m.on_actions, None);
        assert_eq!(m.chosen_row(), Some(Row::Slider(0)));
    }

    #[test]
    fn hair_tones_move_the_colour() {
        let c = [100, 60, 30];
        assert!(tone_reading(tone(c, 0, 20.0), 0) > tone_reading(c, 0));
        assert!(tone_reading(tone(c, 1, 20.0), 1) > tone_reading(c, 1));
        assert!(tone_reading(tone(c, 2, 20.0), 2) > tone_reading(c, 2));
        assert_eq!(tone([250, 250, 250], 0, 20.0), [255, 255, 255]);
    }

    #[test]
    fn the_surgeon_does_the_face_only() {
        let start = Look { head_parts: vec![FormId(50)], ..look() };
        let mut m = Model::of_kind(data(), start.clone(), RaceMenu::Surgeon);
        assert_eq!((m.tab, m.title()), (Tab::Basics, "SURGEON"));
        // No sex or origin; a starting face and the age.
        assert_eq!(m.rows(), vec![Row::Preset, Row::Slider(0)]);
        assert_eq!(m.tabs(), &SURGEON_TABS);
        assert!(!m.set_tab(Tab::Hair), "no Hair tab");
        // The brows are on the Eyes tab.
        m.set_tab(Tab::Eyes);
        assert_eq!(&m.rows()[..2], &[Row::Eyes, Row::Brows]);
        // The beard's shading is the skin's, with no Hair tab.
        assert!(m.groups(Tab::Skin).contains(&"Beard".to_string()));
        // A starting face brings its face, eyes and brows, not its hair or beard.
        assert!(m.adjust_row(Row::Preset, 1, false));
        assert_eq!(m.look.facegen.fggs[3], 2.0);
        assert_eq!(m.look.eyes, Some(FormId(31)));
        assert_eq!((m.look.hair, m.look.hair_color), (start.hair, start.hair_color));
        assert!(!m.look.head_parts.contains(&FormId(40)), "not the preset's beard");
        // Random: a new face; the hair stays.
        let mut rng = <rand_chacha::ChaCha8Rng as rand_core::SeedableRng>::seed_from_u64(5);
        m.randomise(&mut rng);
        assert_eq!((m.look.hair, m.look.hair_color, m.look.race, m.look.female), (start.hair, start.hair_color, start.race, start.female));
    }

    #[test]
    fn the_barber_does_the_hair_only() {
        let mut m = Model::of_kind(data(), Look { head_parts: vec![FormId(50)], ..look() }, RaceMenu::Barber);
        assert_eq!((m.tab, m.title()), (Tab::Hair, "BARBER"));
        assert_eq!(m.rows(), vec![Row::Hair, Row::HairColour, Row::HairTone(0), Row::HairTone(1), Row::HairTone(2), Row::FacialHair]);
        // No other tab: not the sex, race, face or eyes.
        assert!(!m.set_tab(Tab::Basics) && !m.set_tab(Tab::Face));
        assert!(!m.step_tab(1));
        assert_eq!(m.tab, Tab::Hair);
        // Random changes the hair, its colour and the beard; the face, eyes and brows stay.
        let mut rng = <rand_chacha::ChaCha8Rng as rand_core::SeedableRng>::seed_from_u64(3);
        m.randomise(&mut rng);
        assert_eq!(m.look.facegen, look().facegen);
        assert_eq!(m.look.eyes, look().eyes);
        assert!(m.look.head_parts.contains(&FormId(50)), "the brows stay");
        assert_eq!(m.look.hair, Some(FormId(11)));
        assert_eq!(m.look.hair_color, Some([200, 180, 90]));
        assert!(m.look.head_parts.contains(&FormId(40)), "the authored face's beard");
        assert_eq!((m.look.race, m.look.female), (look().race, look().female));
    }

    #[test]
    fn the_creator_opens_on_the_face() {
        let mut m = Model::new(data(), look());
        assert_eq!(m.tab, Tab::Basics);
        let opening = m.camera();
        m.set_tab(Tab::Face);
        let face = m.camera();
        assert!((opening.distance - face.distance).abs() < 1e-3 && (opening.drop - face.drop).abs() < 1e-3, "{opening:?} {face:?}");
        assert!((opening.pitch - face.pitch).abs() < 1e-5);
        assert_eq!(m.view.turn, 0.0, "no turn carried into the other tabs");
        // Back on Basics after another tab: its own framing, the whole body.
        m.set_tab(Tab::Basics);
        assert_eq!(m.camera().distance, FAR);
    }

    #[test]
    fn the_player_stands_left_of_the_panel() {
        // The camera steps to its right, so the player stands left of the picture's middle.
        let mut m = Model::new(data(), look());
        assert!(m.camera().shift < 0.0);
        // Panning the picture right moves the camera left, on top of that step.
        let before = m.camera().shift;
        m.view.pan_by(3.0, 0.0);
        assert!((m.camera().shift - before - 3.0).abs() < 1e-4);
    }

    #[test]
    fn the_camera_follows_the_tab() {
        let mut m = Model::new(data(), look());
        m.view = View::default();
        let body = m.camera();
        assert_eq!(body.distance, FAR);
        m.set_tab(Tab::Face);
        let face = m.camera();
        assert_eq!(face.distance, NEAR);
        // The whole body is looked at from afar, the face from near.
        assert!(body.drop > 40.0 && face.drop < 5.0);
        // A new tab starts its own zoom and pan again, but keeps the turn.
        m.view.zoom_by(2.0);
        m.view.pan_by(5.0, 5.0);
        m.view.orbit(0.5, 0.0);
        m.set_tab(Tab::Basics);
        assert_eq!((m.view.zoom, m.view.pan), (1.0, [0.0, 0.0]));
        assert!((m.view.turn - 0.5).abs() < 1e-6);
    }

    #[test]
    fn zooming_in_on_the_body_aims_at_the_face() {
        let mut v = View::default();
        let f = Tab::Basics.framing();
        let far = v.camera(f);
        v.zoom_by(0.3);
        let near = v.camera(f);
        assert!(near.distance < far.distance);
        assert!(near.drop < far.drop - 30.0, "{} {}", near.drop, far.drop);
        assert_eq!(aim_drop(NEAR), FACE_DROP);
        assert_eq!(aim_drop(1000.0), BODY_DROP);
    }

    #[test]
    fn the_view_stays_within_its_limits() {
        let mut v = View::default();
        let f = Tab::Face.framing();
        v.zoom_by(0.01);
        assert_eq!(v.camera(f).distance, MIN_DISTANCE);
        v.zoom_by(1000.0);
        assert_eq!(v.camera(f).distance, MAX_DISTANCE);
        // The clamp is kept: one step back in comes back at once.
        v.zoom_by(0.5);
        assert!((v.camera(f).distance - MAX_DISTANCE / 2.0).abs() < 1e-3);
        v.orbit(0.0, 5.0);
        assert_eq!(v.camera(f).pitch, MAX_PITCH);
        v.orbit(0.0, -0.1);
        assert!((v.camera(f).pitch - (MAX_PITCH - 0.1)).abs() < 1e-5);
        // All the way round and on.
        v.orbit(7.0, 0.0);
        assert!(v.turn.abs() <= std::f32::consts::PI);
        v.pan_by(500.0, -500.0);
        assert_eq!(v.pan, [PAN_SIDE, -PAN_UP]);
    }
}
