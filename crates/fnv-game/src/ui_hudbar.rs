//! SUP's HUD bars (task T9, docs/spec-notes/menu-xml.md §9): meters a script makes on the
//! HUD "without creating and injecting XML files", each a structure of four tiles,
//! "Meter" (image), "Frame" (image), "Text" (text) and "ImageEx" (image) [documented:
//! GECK wiki, SUP HUD Bar Functions]. NVForever keeps each bar here and writes it into a
//! `rect` it injects into `HUDMainMenu` (so the renderer draws it as a mod's tiles); the
//! bar is written again after every change and after the tree is built again.
//!
//! Bars belong to the calling plugin: two mods may name bars alike; called from the
//! console, the first bar of the name is taken [documented: the category page]. Built:
//! the bars' making, values, look and debug output, and the visibility conditions (bar
//! traits 0 and 21-27) with their gradual vanishing and appearing (41-44, 61-64), stepped
//! each frame by [`tick`] (docs/spec-notes/hud-bars.md). Not built: the texture sizes read
//! from the files. The script-variable and UDF values are `crate::nvse_sup7`'s. The
//! rotation traits are written into the tiles but the tile renderer turns no tile yet
//! (`engine/nvf-play/src/tiles/layout.rs`), so HudBarSetRotation shows nothing until it
//! does.

use crate::game::Game;
use crate::ui_more::{finite, num, opt, text};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_ui::{MemSource, Source, TileId, Value};
use std::collections::BTreeMap;

/// The menu the bars are made in [design: SUP's page names the HUD; its tiles are not
/// documented].
pub const HUD: &str = "HUDMainMenu";

/// The elements by number: 0 Meter, 1 Frame, 2 Text, 3 ImageEx; 10 is all of them (or
/// the bar, for HudBarSetPos and HudBarGetVisible) [documented: the HudBarSet… pages].
pub const ELEMENTS: [&str; 4] = ["Meter", "Frame", "Text", "ImageEx"];
pub const ALL: i64 = 10;

/// One element's look.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// From the bar's place [documented: HudBarSetPos, "Elements positions are locused to
    /// HUD Bar position"].
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub alpha: f64,
    /// 100 [documented: HudBarSetDepth, "Default depth of UI elements is 100"].
    pub depth: f64,
    pub zoom: Option<f64>,
    /// `rotateangle`, `rotateaxisx`, `rotateaxisy` [documented: HudBarSetRotation].
    pub rotate: [f64; 3],
    pub rgb: Option<[f64; 3]>,
    pub systemcolor: f64,
    pub visible: bool,
    pub texture: String,
    pub indent: f64,
    /// The drawn picture's size (HudBarSetSizeAlt).
    pub alt: (f64, f64),
    pub font: Option<f64>,
    pub justify: Option<f64>,
}

impl Element {
    /// [hypothesis] Alpha 255; the system colour 1, the interface's own (HudBarSetColor
    /// needs it set to 0 first, so it is not 0 by default [documented]); the Meter and
    /// the Text shown, the Frame and ImageEx hidden (the category page's examples call
    /// HudBarSetVisible for the Frame only).
    fn new(k: usize) -> Element {
        Element {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            alpha: 255.0,
            depth: 100.0,
            zoom: None,
            rotate: [0.0; 3],
            rgb: None,
            systemcolor: 1.0,
            visible: k == 0 || k == 2,
            texture: String::new(),
            indent: 0.0,
            alt: (0.0, 0.0),
            font: None,
            justify: None,
        }
    }

    /// Where its picture ends to the right and below, for HudBarSetAutoPos: its indent and
    /// drawn size when set, else its size [hypothesis: the page asks for both or neither].
    fn right(&self) -> f64 {
        self.x + if self.alt.0 > 0.0 { self.indent + self.alt.0 } else { self.width }
    }

    fn bottom(&self) -> f64 {
        self.y + if self.alt.1 > 0.0 { self.alt.1 } else { self.height }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// The calling plugin, lower case ("" from the console).
    pub owner: String,
    pub name: String,
    /// Numbers the bar's tile, `NVFHudBar<id>` [design].
    pub id: u32,
    /// 0 horizontal, 1 vertical, 2 alpha, 3 value text, 4 percentage text, 5 size
    /// [documented: the category page].
    pub kind: u8,
    pub x: f64,
    pub y: f64,
    /// Bars are hidden after creation [documented: HudBarCreate notes].
    pub shown: bool,
    pub elements: [Element; 4],
    /// The main text, the prefix and the postfix [documented: HudBarSetTextString].
    pub texts: [String; 3],
    /// HudBarSetBarTrait's traits by number.
    pub traits: BTreeMap<i64, f64>,
    /// HudBarSetMeterTrait's: alpha at 0 % and 100 %, show the max value, min and max
    /// width, min and max height [documented: HudBarSetMeterTrait; defaults 0 and 255].
    pub meter: [f64; 7],
    pub value: f64,
    pub max: f64,
    /// The meter's share, 0 to 1.
    pub share: f64,
    pub item: Option<u32>,
    pub list: Option<u32>,
    /// How far the bar is there under its conditions: 1 shown, 0 hidden by them, between
    /// while it vanishes or appears gradually (traits 41 and 61) [design].
    pub presence: f64,
    /// The way the last change of `presence` went: true appearing (traits 61-64), false
    /// vanishing (41-44); it picks the movement traits [design].
    pub appearing: bool,
    /// Where the current change started: its `presence` and the bar's shift then (x, y),
    /// so a change that turns back half way moves on from where the bar is [design].
    pub from: [f64; 3],
    /// The bar was last hidden by trait 0 or 27 (at once): shown again, it is at once too
    /// [design].
    pub instant_hide: bool,
}

/// What the visibility conditions read from the game, gathered once a frame ([`tick`])
/// or when HudBarGetVisible asks. All of it is the player's [documented: HudBarSetBarTrait,
/// "player is sneaking", "player has their weapon out" and so on].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Situation {
    /// [`Game::in_menu`]: a menu open or a conversation running.
    pub menu_mode: bool,
    /// The movement group of DisablePlayerControls (or any plugin's
    /// DisablePlayerControlsAlt) is held (`Game::controls_locked`, index 0).
    pub movement_disabled: bool,
    pub sneaking: bool,
    /// IsInCombat's answer for the player (`awareness::is_in_combat`).
    pub combat: bool,
    /// Aiming down the sights or the scope: the sights up as the engine tells it
    /// (`nvse_jip2::PlayerTold::sights_up`), not while blocking with a melee weapon or
    /// while a reload holds the sights down [design: the engine's sights-up test].
    pub aiming: bool,
    pub weapon_out: bool,
    pub first_person: bool,
    /// The base forms the player has equipped.
    pub equipped: Vec<u32>,
}

impl Situation {
    pub fn read(g: &Game) -> Situation {
        let p = fnv_sim::FormId::PLAYER_REF;
        let equipped = g.world.equipment(p).unwrap_or_default().into_iter().map(|f| f.0).collect();
        Situation {
            menu_mode: g.in_menu(),
            movement_disabled: g.controls_locked()[0],
            sneaking: g.is_sneaking(p),
            combat: g.awareness.hunted(p),
            aiming: g.jip2.told.sights_up,
            weapon_out: g.extra.get(&p).is_some_and(|e| e.weapon_out),
            first_person: g.senses.first_person,
            equipped,
        }
    }
}

/// Which of a bar's conditions hide it now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hidden {
    /// By one of traits 21-26, the ones that may vanish and appear gradually
    /// [documented: HudBarSetBarTrait, traits 41 and 61, "hidden via traits(21-26)"].
    pub gradual: bool,
    /// By trait 0 (menu mode) or 27 (camera): at once, the fades being only for 21-26
    /// [hypothesis: read from the same words].
    pub instant: bool,
}

impl Hidden {
    pub fn any(self) -> bool {
        self.gradual || self.instant
    }
}

impl Bar {
    /// The bar's rect: `HUDMainMenu/NVFHudBar<id>`.
    pub fn path(&self) -> String {
        format!("{HUD}/NVFHudBar{}", self.id)
    }

    /// A bar trait's value: the one set, else fUpdateTimer's 0.1 s [documented:
    /// HudBarSetValueScriptVar on the category page, "default is 0.1s"] or 0
    /// [hypothesis].
    pub fn bar_trait(&self, k: i64) -> f64 {
        self.traits.get(&k).copied().unwrap_or(if k == 2 { 0.1 } else { 0.0 })
    }

    /// A trait as the whole number its page lists.
    fn code(&self, k: i64) -> i64 {
        self.bar_trait(k) as i64
    }

    /// Which conditions hide the bar in `s`; `item_on` says whether its item or any member
    /// of its form list is equipped (trait 25, [`item_equipped`]).
    ///
    /// [documented: HudBarSetBarTrait] 0: 1 shown only in game mode; 21: 1 hidden while
    /// the movement controls are disabled; 22, 23, 24: 1 hidden while sneaking, in combat,
    /// aiming, 2 shown only then; 25: 1 hidden unless the item is equipped; 26: 1 hidden
    /// with the weapon out, 2 hidden without it; 27: 1 hidden in third person, 2 in first
    /// person.
    ///
    /// Trait 0's page gives the value 1 to both "Show only in Gamemode" and "Show only in
    /// menumode" (see docs/spec-notes/hud-bars.md §2); 1 is read as the first and 2 as the second, the way traits
    /// 22-24 and 26 number their two alternatives. A value a page does not list hides
    /// nothing [design].
    ///
    /// [design] The conditions combine as vetoes: the bar shows only when none of those
    /// set hides it. The page's note says "any combination is possible" and gives "appear
    /// only when aiming in combat" (22-24's value 2 on 23 and 24), then "in other cases it
    /// will be shown", which no rule gives together with the first half; the veto rule
    /// keeps the first half (each "only when" must hold) and so hides the bar outside
    /// combat or when not aiming. docs/spec-notes/hud-bars.md §2.
    pub fn hidden_by(&self, s: &Situation, item_on: bool) -> Hidden {
        let only_or_not = |k: i64, now: bool| match self.code(k) {
            1 => now,
            2 => !now,
            _ => false,
        };
        let gradual = (self.code(21) == 1 && s.movement_disabled)
            || only_or_not(22, s.sneaking)
            || only_or_not(23, s.combat)
            || only_or_not(24, s.aiming)
            || (self.code(25) == 1 && !item_on)
            || only_or_not(26, s.weapon_out);
        let instant = match self.code(0) {
            1 => s.menu_mode,
            2 => !s.menu_mode,
            _ => false,
        } || match self.code(27) {
            1 => !s.first_person,
            2 => s.first_person,
            _ => false,
        };
        Hidden { gradual, instant }
    }

    /// One frame of `dt` seconds under the conditions `h`: `presence` moves towards 1
    /// (none hides the bar) or 0. [documented: HudBarSetBarTrait] With 41 set the bar
    /// vanishes over 44's seconds, with 61 set it appears over 64's, otherwise at once.
    /// [design] At once: a bar hidden by HudBarShowBar (nothing is drawn to fade), a bar
    /// hidden by trait 0 or 27, a bar shown again after one of those hid it last, a timer
    /// of 0 or less. A change that turns back half way goes back from where it is, at the
    /// other way's speed, its place moving on from where it is (no jump). The fades step
    /// by `dt` only when [`tick`] runs. True when `presence` changed.
    pub fn step_presence(&mut self, h: Hidden, dt: f64) -> bool {
        let was = self.presence;
        let target = if h.any() { 0.0 } else { 1.0 };
        if h.any() {
            self.instant_hide = h.instant;
        }
        let appearing = if target > self.presence {
            true
        } else if target < self.presence {
            false
        } else {
            self.appearing
        };
        if appearing != self.appearing {
            // Half way the change starts from the bar's shift now; at an end from the
            // way's own start (appearing from 62/63's shift, vanishing from its place).
            let d = if self.presence > 0.0 && self.presence < 1.0 {
                self.offset()
            } else if appearing {
                self.shift(62, 63)
            } else {
                (0.0, 0.0)
            };
            self.from = [self.presence, d.0, d.1];
            self.appearing = appearing;
        }
        if !self.shown || h.instant || (!h.any() && self.instant_hide) {
            self.presence = target;
        } else if target != self.presence {
            let (on, timer) = if self.appearing { (61, 64) } else { (41, 44) };
            let t = self.bar_trait(timer);
            self.presence = if self.bar_trait(on) != 0.0 && t > 0.0 && t.is_finite() {
                let step = dt.max(0.0) / t;
                if self.appearing { (self.presence + step).min(1.0) } else { (self.presence - step).max(0.0) }
            } else {
                target
            };
        }
        self.presence != was
    }

    /// Where the conditions `h` put the bar, at once and at rest (no change under way).
    pub fn settle(&mut self, h: Hidden) {
        let hidden = h.any();
        self.presence = if hidden { 0.0 } else { 1.0 };
        self.appearing = !hidden;
        self.from = [self.presence, 0.0, 0.0];
        if hidden {
            self.instant_hide = h.instant;
        }
    }

    /// A way's whole shift: [documented: HudBarSetBarTrait] 42 (62) 1 horizontally, 2
    /// vertically, by 43's (63's) pixels, positive to the right (down; see
    /// docs/spec-notes/hud-bars.md §3).
    fn shift(&self, mode: i64, by: i64) -> (f64, f64) {
        let d = self.bar_trait(by);
        match self.code(mode) {
            1 => (d, 0.0),
            2 => (0.0, d),
            _ => (0.0, 0.0),
        }
    }

    /// The bar's shift from its place now: vanishing it moves from where the change
    /// started towards 42/43's shift, appearing from where it started (62/63's shift
    /// when it starts hidden) to its place, linear in `presence` [design].
    pub fn offset(&self) -> (f64, f64) {
        let (end, t) = if self.appearing { ((0.0, 0.0), 1.0) } else { (self.shift(42, 43), 0.0) };
        let p0 = self.from[0];
        if p0 == t {
            return end;
        }
        let k = ((self.presence - t) / (p0 - t)).clamp(0.0, 1.0);
        (end.0 + (self.from[1] - end.0) * k, end.1 + (self.from[2] - end.1) * k)
    }

    /// The bar's place on the screen with its shift.
    pub fn place(&self) -> (f64, f64) {
        let (dx, dy) = self.offset();
        (self.x + dx, self.y + dy)
    }

    /// An element's alpha before the bar's `presence`: type 2's Meter from the share
    /// ([`Bar::meter`]), the others their own.
    pub fn element_alpha(&self, k: usize) -> f64 {
        if k == 0 && self.kind == 2 {
            self.meter[0] + (self.meter[1] - self.meter[0]) * self.share
        } else {
            self.elements[k].alpha
        }
    }

    /// The Text element's string: for types 3 and 4 the value [documented: "Value/MaxValue"
    /// with show-max on, "Player Inventory weight:50/210 wg" from prefix, value, max and
    /// postfix; hypothesis: type 4 is the rounded percentage between prefix and postfix,
    /// the page's example adds the "%" as a postfix], else the main text.
    pub fn text(&self) -> String {
        let n = |v: f64| Value::Num(v).text();
        match self.kind {
            3 => {
                let max = if self.meter[2] != 0.0 { format!("/{}", n(self.max)) } else { String::new() };
                format!("{}{}{}{}", self.texts[1], n(self.value), max, self.texts[2])
            }
            4 => format!("{}{}{}", self.texts[1], n((self.share * 100.0).round()), self.texts[2]),
            _ => self.texts[0].clone(),
        }
    }

    /// The Meter's look from the share: type 2's alpha between the meter traits' 0 % and
    /// 100 % values [documented: the category page's alpha example]; [hypothesis] type 0's
    /// width and type 1's height are the indent plus the share of the rest, type 5's
    /// width and height between the meter traits' minimum and maximum.
    pub fn meter(&self) -> Element {
        let mut m = self.elements[0].clone();
        let s = self.share;
        match self.kind {
            0 => m.width = m.indent + (m.width - m.indent).max(0.0) * s,
            1 => m.height = m.indent + (m.height - m.indent).max(0.0) * s,
            2 => m.alpha = self.element_alpha(0),
            5 => {
                m.width = self.meter[3] + (self.meter[4] - self.meter[3]) * s;
                m.height = self.meter[5] + (self.meter[6] - self.meter[5]) * s;
            }
            _ => {}
        }
        m
    }
}

#[derive(Default)]
pub struct Bars {
    pub list: Vec<Bar>,
    seq: u32,
}

/// The calling plugin's bar of a name; from the console the first of the name.
fn find(g: &Game, name: &str) -> Option<usize> {
    let owner = &g.other.plugin;
    g.ui_extra.bars.list.iter().position(|b| b.name.eq_ignore_ascii_case(name) && (owner.is_empty() || b.owner == *owner))
}

/// The id of the calling plugin's bar of a name (from the console the first of the
/// name), for the value sources of `crate::nvse_sup7`.
pub fn bar_id(g: &Game, name: &str) -> Option<u32> {
    find(g, name).map(|i| g.ui_extra.bars.list[i].id)
}

/// A value from a bar's source (SUP's HudBarSetValueScriptVar and HudBarSetValueUDF):
/// with iCalculateValueMax as HudBarSetValueFloat, the value against the max; without
/// it the raw value is the meter's share, 0.00-1.00 as HudBarSetValuePercentage's
/// [documented: "turned into percentage using <MaxValue> if iCalculateValueMax is set
/// to 1. If not - raw value will be passed"; hypothesis: "passed" as the share, the
/// value text showing the raw value]. Written only when it changed.
pub fn set_value(g: &mut Game, i: usize, v: f64, of_max: bool) {
    let b = &mut g.ui_extra.bars.list[i];
    let share = if of_max {
        if b.max != 0.0 { (v / b.max).clamp(0.0, 1.0) } else { 0.0 }
    } else {
        v.clamp(0.0, 1.0)
    };
    if b.value == v && b.share == share {
        return;
    }
    (b.value, b.share) = (v, share);
    apply(g, i);
}

/// Trait 25's test: the bar's item (HudBarSetItem) or any member of its form list
/// (HudBarSetFormList) is equipped [documented: the two pages, "only show up when this
/// item is equipped", "when any item from formlist is equipped"]. [design] With both
/// set, either one is enough (neither page puts one before the other). With neither
/// set, or only an empty list, nothing named can be equipped (see docs/spec-notes/hud-bars.md §2), so trait 25
/// keeps the bar hidden, as its words say ("hide bar when player doesn't have item
/// equipped").
pub fn item_equipped(g: &Game, b: &Bar, s: &Situation) -> bool {
    let on = |f: u32| s.equipped.contains(&f);
    b.item.is_some_and(on) || b.list.is_some_and(|l| crate::functions::refs::list_members(g, fnv_sim::FormId(l)).into_iter().any(|m| on(m.0)))
}

/// The conditions that hide bar `i` now (for HudBarGetVisible, between frames too).
fn hidden_now(g: &Game, i: usize) -> Hidden {
    let s = Situation::read(g);
    let b = &g.ui_extra.bars.list[i];
    b.hidden_by(&s, b.code(25) == 1 && item_equipped(g, b, &s))
}

/// Each frame (`Game::tick`): every bar's conditions read, its `presence` stepped by the
/// frame's real seconds (`GameConfig::frame_seconds`), and the bars that changed written
/// again. [design] The fades go on in menu mode (the Pip-Boy, a conversation: `Game::tick`
/// runs there, game time standing still) and wait with the game under the pause menu and
/// a loading screen, where the engine runs no `Game::tick`
/// (`engine/nvf-play/src/script.rs`).
pub fn tick(g: &mut Game) {
    if g.ui_extra.bars.list.is_empty() {
        return;
    }
    let s = Situation::read(g);
    let dt = g.cfg.frame_seconds;
    for i in 0..g.ui_extra.bars.list.len() {
        let b = &g.ui_extra.bars.list[i];
        let item_on = b.code(25) == 1 && item_equipped(g, b, &s);
        let h = b.hidden_by(&s, item_on);
        if g.ui_extra.bars.list[i].step_presence(h, dt) && !apply_fade(g, i) {
            apply(g, i);
        }
    }
}

/// Writes what a fade changes, the bar's place, its being drawn and the elements' alpha,
/// and nothing else [design]; false when its tiles are not in the tree.
fn apply_fade(g: &mut Game, i: usize) -> bool {
    let b = &g.ui_extra.bars.list[i];
    let ui = &mut g.ui_tree;
    let Some(rect) = ui.find_tile(&b.path()) else { return false };
    let (x, y) = b.place();
    ui.set_trait(rect, "x", Value::Num(x));
    ui.set_trait(rect, "y", Value::Num(y));
    ui.set_trait(rect, "visible", Value::from_bool(b.shown && b.presence > 0.0));
    // By index: no copy of the children for a frame's few writes.
    let count = ui.tile(rect).children.len();
    for c in 0..count {
        let t = ui.tile(rect).children[c];
        if let Some(k) = ELEMENTS.iter().position(|n| ui.tile(t).name == *n) {
            ui.set_trait(t, "alpha", Value::Num(b.element_alpha(k) * b.presence));
        }
    }
    true
}

/// A plugin's name by its load-order index, lower case.
fn plugin_at(g: &Game, index: f64) -> Option<String> {
    g.data.load_order.get(index.max(0.0) as usize).map(|p| p.to_ascii_lowercase())
}

/// Writes a bar into the tree, making its tiles in the HUD when they are not there; false
/// when the HUD is not loaded.
pub fn apply(g: &mut Game, i: usize) -> bool {
    let bar = g.ui_extra.bars.list[i].clone();
    let ui = &mut g.ui_tree;
    let rect = match ui.find_tile(&bar.path()) {
        Some(t) => t,
        None => {
            let xml = format!(
                "<rect name=\"NVFHudBar{}\"> <locus> &true; </locus> <image name=\"Meter\"> </image> <image name=\"Frame\"> </image> <text name=\"Text\"> </text> <image name=\"ImageEx\"> </image> </rect>",
                bar.id
            );
            let empty = MemSource::default();
            let src: &dyn Source = match g.ui_files.as_deref() {
                Some(s) => s,
                None => &empty,
            };
            if !ui.inject_text(src, HUD, &xml) {
                return false;
            }
            match ui.find_tile(&bar.path()) {
                Some(t) => t,
                None => return false,
            }
        }
    };
    // The conditions' gradual change moves the bar and fades every element by its
    // `presence`; at 0 the bar is not drawn [design].
    let (x, y) = bar.place();
    ui.set_trait(rect, "x", Value::Num(x));
    ui.set_trait(rect, "y", Value::Num(y));
    ui.set_trait(rect, "visible", Value::from_bool(bar.shown && bar.presence > 0.0));
    let children: Vec<TileId> = ui.tile(rect).children.clone();
    for (k, name) in ELEMENTS.iter().enumerate() {
        let Some(&t) = children.iter().find(|&&c| ui.tile(c).name == *name) else { continue };
        let e = if k == 0 { bar.meter() } else { bar.elements[k].clone() };
        let set = |ui: &mut fnv_ui::Ui, n: &str, v: Value| ui.set_trait(t, n, v);
        set(ui, "x", Value::Num(e.x));
        set(ui, "y", Value::Num(e.y));
        // The Text's size is its string's unless one was set (the layout measures a text
        // without a `width` or `height`, `engine/nvf-play/src/tiles/layout.rs`); the
        // pictures take theirs, 0 until set [design].
        if k != 2 || e.width > 0.0 {
            set(ui, "width", Value::Num(e.width));
        }
        if k != 2 || e.height > 0.0 {
            set(ui, "height", Value::Num(e.height));
        }
        set(ui, "alpha", Value::Num(e.alpha * bar.presence));
        set(ui, "depth", Value::Num(e.depth));
        set(ui, "visible", Value::from_bool(e.visible));
        set(ui, "systemcolor", Value::Num(e.systemcolor));
        for (r, n) in ["rotateangle", "rotateaxisx", "rotateaxisy"].iter().enumerate() {
            if e.rotate[r] != 0.0 || ui.tile(t).get(n).is_some() {
                set(ui, n, Value::Num(e.rotate[r]));
            }
        }
        if let Some(z) = e.zoom {
            set(ui, "zoom", Value::Num(z));
        }
        if let Some([r, gr, b]) = e.rgb {
            set(ui, "red", Value::Num(r));
            set(ui, "green", Value::Num(gr));
            set(ui, "blue", Value::Num(b));
        }
        if k == 2 {
            set(ui, "string", Value::Str(bar.text()));
            if let Some(f) = e.font {
                set(ui, "font", Value::Num(f));
            }
            // Justify 1 right, 2 center, 3 left, 0 unused [documented: HudBarSetTextTrait],
            // written as the menu files' constants [design].
            match e.justify.map(|j| j as i64) {
                Some(1) => set(ui, "justify", Value::Sym("right".into())),
                Some(2) => set(ui, "justify", Value::Sym("center".into())),
                Some(3) => set(ui, "justify", Value::Sym("left".into())),
                _ => {}
            }
        } else if !e.texture.is_empty() {
            set(ui, "filename", Value::Str(e.texture.clone()));
        }
    }
    true
}

/// The tree was built again: bars with iRemoveOnGameLoad (trait 1) go [documented:
/// HudBarSetBarTrait], the rest are made again in the new HUD.
pub fn rebuilt(g: &mut Game) {
    g.ui_extra.bars.list.retain(|b| b.bar_trait(1) == 0.0);
    for i in 0..g.ui_extra.bars.list.len() {
        // A fade under way is finished at once in the new HUD [design].
        let h = hidden_now(g, i);
        g.ui_extra.bars.list[i].settle(h);
        apply(g, i);
    }
}

/// The element numbers a function takes, with 10 for all of them.
fn targets(e: i64, allowed: &[i64]) -> Vec<usize> {
    if e == ALL && allowed.contains(&ALL) {
        allowed.iter().filter(|&&k| k != ALL).map(|&k| k as usize).collect()
    } else if allowed.contains(&e) && e != ALL {
        vec![e as usize]
    } else {
        Vec::new()
    }
}

/// The debug lines of a bar (SUP writes them to SUPNVSE.log [documented:
/// HudBarGetDebugInfo]; NVForever to its own log).
fn debug_lines(b: &Bar) -> Vec<String> {
    vec![format!(
        "HUD bar {:?} of {:?}: type {}, at {} {}, shown {}, value {} of {} ({}), tile {}",
        b.name, b.owner, b.kind, b.x, b.y, b.shown, b.value, b.max, b.share, b.path()
    )]
}

/// A HudBar function by lower-case name; `None` for any other (and for
/// HudBarSetValueScriptVar and HudBarSetValueUDF, built in `crate::nvse_sup7`).
/// A non-finite number in a place, size, colour or trait writes 0; the value, max and
/// percentage setters refuse one with -1 [design: `ui_more::finite`].
pub(crate) fn call(g: &mut Game, lname: &str, args: &[XVal]) -> Option<Result<XVal, String>> {
    if !lname.starts_with("hudbar") {
        return None;
    }
    let name = text(args, 0);
    let found = find(g, &name);
    // 1 for success, -1 for failure [documented: HudBarSetBarTrait, HudBarSetColor and the
    // value setters; hypothesis for the other setters, whose pages give "success:int"].
    let ok = |b: bool| XVal::Num(if b { 1.0 } else { -1.0 });
    let zero = XVal::Num(0.0);
    // The element-wise setters: change the elements a number names, write the bar.
    let set_elements = |g: &mut Game, allowed: &[i64], f: &dyn Fn(&mut Element)| -> XVal {
        let Some(i) = found else { return ok(false) };
        let ks = targets(num(args, 1) as i64, allowed);
        if ks.is_empty() {
            return ok(false);
        }
        for k in ks {
            f(&mut g.ui_extra.bars.list[i].elements[k]);
        }
        apply(g, i);
        ok(true)
    };
    let imgs: &[i64] = &[0, 1, 3, ALL];
    let every: &[i64] = &[0, 1, 2, 3, ALL];
    let (a2, a3, a4) = (finite(args, 2), finite(args, 3), finite(args, 4));
    Some(Ok(match lname {
        // `name x y type`; false when the plugin has a bar of the name or the type is
        // not one of 0-5 [hypothesis].
        "hudbarcreate" => {
            let kind = num(args, 3);
            if found.is_some_and(|i| g.ui_extra.bars.list[i].owner == g.other.plugin) || !(0.0..=5.0).contains(&kind) {
                return Some(Ok(zero));
            }
            g.ui_extra.bars.seq += 1;
            let bar = Bar {
                owner: g.other.plugin.clone(),
                name,
                id: g.ui_extra.bars.seq,
                kind: kind as u8,
                x: finite(args, 1),
                y: finite(args, 2),
                shown: false,
                elements: [Element::new(0), Element::new(1), Element::new(2), Element::new(3)],
                texts: Default::default(),
                traits: BTreeMap::new(),
                meter: [0.0, 255.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                value: 0.0,
                // [hypothesis: 100; the page's example "d" sets 50 with no max and calls
                // it the bar above at 50 %].
                max: 100.0,
                share: 0.0,
                item: None,
                list: None,
                presence: 1.0,
                appearing: true,
                from: [1.0, 0.0, 0.0],
                instant_hide: false,
            };
            g.ui_extra.bars.list.push(bar);
            let i = g.ui_extra.bars.list.len() - 1;
            apply(g, i);
            XVal::Num(1.0)
        }
        "hudbarexists" => XVal::Num(if found.is_some() { 1.0 } else { 0.0 }),
        "hudbardestroy" => {
            if let Some(i) = found {
                let bar = g.ui_extra.bars.list.remove(i);
                if let Some(t) = g.ui_tree.find_tile(&bar.path()) {
                    g.ui_tree.unload_tile(t);
                }
            }
            zero
        }
        // The calling plugin's bars [documented: HudBarDestroyAllBars].
        "hudbardestroyallbars" => {
            let owner = g.other.plugin.clone();
            let (gone, kept): (Vec<Bar>, Vec<Bar>) = std::mem::take(&mut g.ui_extra.bars.list).into_iter().partition(|b| b.owner == owner);
            g.ui_extra.bars.list = kept;
            for b in gone {
                if let Some(t) = g.ui_tree.find_tile(&b.path()) {
                    g.ui_tree.unload_tile(t);
                }
            }
            zero
        }
        // The calling plugin's bar names; with 1 every bar: [names, owning mods' names,
        // their numbers] [documented: HudBarGetAllBars]. A plugin not in the load order
        // numbers -1 [hypothesis].
        "hudbargetallbars" => {
            let all = num(args, 0) != 0.0;
            let owner = g.other.plugin.clone();
            let bars: Vec<Bar> = g.ui_extra.bars.list.iter().filter(|b| all || b.owner == owner).cloned().collect();
            let list = |g: &mut Game, items: Vec<XVal>| {
                let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
                XVal::Array(g.ext.new_array(ArrayKind::List, entries))
            };
            let names = list(g, bars.iter().map(|b| XVal::Str(b.name.clone())).collect());
            if all {
                let mods = list(g, bars.iter().map(|b| XVal::Str(b.owner.clone())).collect());
                let numbers: Vec<XVal> = bars.iter().map(|b| XVal::Num(g.data.load_order.iter().position(|p| p.eq_ignore_ascii_case(&b.owner)).map_or(-1.0, |n| n as f64))).collect();
                let numbers = list(g, numbers);
                list(g, vec![names, mods, numbers])
            } else {
                names
            }
        }
        // `name trait [modIndex]`: another mod's bars are read by its index [documented:
        // HudBarGetBarTrait]. 0 without the bar [hypothesis].
        "hudbargetbartrait" => {
            let i = match opt(args, 2).and_then(|m| plugin_at(g, m)) {
                Some(owner) => g.ui_extra.bars.list.iter().position(|b| b.owner == owner && b.name.eq_ignore_ascii_case(&name)),
                None => found,
            };
            XVal::Num(i.map_or(0.0, |i| g.ui_extra.bars.list[i].bar_trait(num(args, 1) as i64)))
        }
        "hudbarsetbartrait" => match found {
            Some(i) => {
                g.ui_extra.bars.list[i].traits.insert(num(args, 1) as i64, a2);
                ok(true)
            }
            None => ok(false),
        },
        "hudbargetdebuginfo" | "hudbargetdebuginfoall" => {
            let lines: Vec<String> = if lname == "hudbargetdebuginfo" { found.map(|i| debug_lines(&g.ui_extra.bars.list[i])).unwrap_or_default() } else { g.ui_extra.bars.list.iter().flat_map(debug_lines).collect() };
            crate::ui_more::emit(g, true, lines);
            zero
        }
        // The path of an element, 10 the bar's [documented: HudBarGetElementUIPath; design:
        // the tiles' names].
        "hudbargetelementuipath" => match found {
            Some(i) => {
                let b = &g.ui_extra.bars.list[i];
                let e = num(args, 1) as i64;
                XVal::Str(match e {
                    0..=3 => format!("{}/{}", b.path(), ELEMENTS[e as usize]),
                    ALL => b.path(),
                    _ => String::new(),
                })
            }
            None => XVal::Str(String::new()),
        },
        // `name element [modIndex]`; 10: 1 shown, 0 hidden by HudBarShowBar, -1 hidden by
        // its conditions [documented: HudBarGetVisible, "if it's set to be hidden in First
        // person"]. Hidden both ways is 0 (HudBarShowBar's; see docs/spec-notes/hud-bars.md §4); every condition
        // (traits 0, 21-27) gives -1, read now, so -1 from the moment a condition hides the
        // bar, while it still vanishes, and 1 from the moment none does, while it still
        // appears. The elements 0, 1 and 3 answer their own HudBarSetVisible. 0 without the
        // bar [hypothesis].
        "hudbargetvisible" => {
            let i = match opt(args, 2).and_then(|m| plugin_at(g, m)) {
                Some(owner) => g.ui_extra.bars.list.iter().position(|b| b.owner == owner && b.name.eq_ignore_ascii_case(&name)),
                None => found,
            };
            XVal::Num(match (i, num(args, 1) as i64) {
                (Some(i), ALL) if !g.ui_extra.bars.list[i].shown => 0.0,
                (Some(i), ALL) => {
                    if hidden_now(g, i).any() {
                        -1.0
                    } else {
                        1.0
                    }
                }
                (Some(i), e @ (0 | 1 | 3)) => f64::from(u8::from(g.ui_extra.bars.list[i].elements[e as usize].visible)),
                _ => 0.0,
            })
        }
        "hudbarsetalpha" => set_elements(g, imgs, &|e| e.alpha = a2),
        "hudbarsetdepth" => set_elements(g, imgs, &|e| e.depth = a2),
        "hudbarsetindent" => set_elements(g, imgs, &|e| e.indent = a2),
        "hudbarsetvisible" => set_elements(g, imgs, &|e| e.visible = a2 != 0.0),
        "hudbarsetzoom" => set_elements(g, every, &|e| e.zoom = Some(a2)),
        "hudbarsetcolor" => set_elements(g, every, &|e| e.rgb = Some([a2, a3, a4])),
        "hudbarsetsystemcolor" => set_elements(g, &[0, 1, 2, 3], &|e| e.systemcolor = a2),
        "hudbarsetsize" => set_elements(g, &[0, 1, 3], &|e| (e.width, e.height) = (a2, a3)),
        "hudbarsetsizealt" => set_elements(g, &[0, 1, 3], &|e| e.alt = (a2, a3)),
        // `name element type value`, type 0 rotateangle, 1 rotateaxisx, 2 rotateaxisy
        // [documented: HudBarSetRotation].
        "hudbarsetrotation" => match a2 as i64 {
            r @ 0..=2 => set_elements(g, every, &|e| e.rotate[r as usize] = a3),
            _ => ok(false),
        },
        // `name element trait value`: element 0 is the Text, trait 0 the font, 1 the
        // justification [documented: HudBarSetTextTrait].
        "hudbarsettexttrait" => match (found, num(args, 1) as i64, a2 as i64) {
            (Some(i), 0, t @ (0 | 1)) => {
                let e = &mut g.ui_extra.bars.list[i].elements[2];
                if t == 0 {
                    e.font = Some(a3);
                } else {
                    e.justify = Some(a3);
                }
                apply(g, i);
                ok(true)
            }
            _ => ok(false),
        },
        // `name element path [calculateSize]` [documented: HudBarSetTexture]; reading the
        // size off the texture is not built.
        "hudbarsettexture" => {
            let path = text(args, 2);
            set_elements(g, &[0, 1, 3], &|e| e.texture = path.clone())
        }
        // `name element x y`, 10 the bar's place on the screen [documented: HudBarSetPos].
        "hudbarsetpos" => match found {
            Some(i) if num(args, 1) as i64 == ALL => {
                let b = &mut g.ui_extra.bars.list[i];
                (b.x, b.y) = (a2, a3);
                apply(g, i);
                ok(true)
            }
            _ => set_elements(g, &[0, 1, 2, 3], &|e| (e.x, e.y) = (a2, a3)),
        },
        // `name element text`: 0 the main text, 1 the prefix, 2 the postfix [documented:
        // HudBarSetTextString].
        "hudbarsettextstring" => match (found, num(args, 1) as i64) {
            (Some(i), k @ 0..=2) => {
                g.ui_extra.bars.list[i].texts[k as usize] = text(args, 2);
                apply(g, i);
                ok(true)
            }
            _ => ok(false),
        },
        "hudbarsetmetertrait" => match (found, num(args, 1) as i64) {
            (Some(i), k @ 0..=6) => {
                g.ui_extra.bars.list[i].meter[k as usize] = a2;
                apply(g, i);
                ok(true)
            }
            _ => ok(false),
        },
        // The value against the max [documented: HudBarSetValueFloat, "Value/<MaxValue>";
        // type 3 shows the raw value].
        "hudbarsetvaluefloat" | "hudbarsetvaluemax" => match (found, num(args, 1)) {
            // A value that is no number (a script's bad arithmetic) is refused [design].
            (Some(i), v) if v.is_finite() => {
                let b = &mut g.ui_extra.bars.list[i];
                if lname == "hudbarsetvaluefloat" {
                    b.value = v;
                } else {
                    b.max = v;
                }
                b.share = if b.max != 0.0 { (b.value / b.max).clamp(0.0, 1.0) } else { 0.0 };
                apply(g, i);
                ok(true)
            }
            _ => ok(false),
        },
        // 0.00-1.00, "normalized" when outside; not for type 3 [documented:
        // HudBarSetValuePercentage]. The value follows as that share of the max
        // [hypothesis]. A share that is no number is refused [design].
        "hudbarsetvaluepercentage" => match (found, num(args, 1)) {
            (Some(i), v) if g.ui_extra.bars.list[i].kind != 3 && v.is_finite() => {
                let b = &mut g.ui_extra.bars.list[i];
                b.share = v.clamp(0.0, 1.0);
                b.value = b.share * b.max;
                apply(g, i);
                ok(true)
            }
            _ => ok(false),
        },
        // [documented: HudBarSetItem, HudBarSetFormList: no form removes it; used by
        // iDisableWhenItemNotEquipped, trait 25, [`item_equipped`]].
        "hudbarsetitem" | "hudbarsetformlist" => match found {
            Some(i) => {
                let f = match args.get(1) {
                    Some(XVal::Ref(r)) if *r != 0 => Some(*r),
                    _ => None,
                };
                let b = &mut g.ui_extra.bars.list[i];
                if lname == "hudbarsetitem" {
                    b.item = f;
                } else {
                    b.list = f;
                }
                ok(true)
            }
            None => ok(false),
        },
        // A bar shown starts where its conditions are, without a fade [design: no frame
        // of a bar its conditions hide]. Hidden, its value is no longer read
        // (`crate::nvse_sup7`) [documented: HudBarShowBar, "Bar value calculations will
        // stop processing"].
        "hudbarshowbar" => match found {
            Some(i) => {
                let h = hidden_now(g, i);
                let b = &mut g.ui_extra.bars.list[i];
                b.shown = num(args, 1) != 0.0;
                b.settle(h);
                apply(g, i);
                XVal::Num(1.0)
            }
            None => zero,
        },
        // The Text beside the pictures: 0 to their right, 1 below, 2 above [documented:
        // HudBarSetAutoPos]. [hypothesis] The pictures are the visible Meter, Frame and
        // ImageEx; to the right at the top of the highest, below and above at the left of
        // the leftmost; above by the Text's height.
        "hudbarsetautopos" => match found {
            Some(i) => {
                let b = &mut g.ui_extra.bars.list[i];
                let pics: Vec<Element> = [0, 1, 3].iter().map(|&k| b.elements[k].clone()).filter(|e| e.visible).collect();
                if !pics.is_empty() {
                    let left = pics.iter().map(|e| e.x).fold(f64::INFINITY, f64::min);
                    let top = pics.iter().map(|e| e.y).fold(f64::INFINITY, f64::min);
                    let right = pics.iter().map(Element::right).fold(f64::NEG_INFINITY, f64::max);
                    let bottom = pics.iter().map(Element::bottom).fold(f64::NEG_INFINITY, f64::max);
                    let t = &mut b.elements[2];
                    (t.x, t.y) = match num(args, 1) as i64 {
                        1 => (left, bottom),
                        2 => (left, top - t.height),
                        _ => (right, top),
                    };
                }
                apply(g, i);
                zero
            }
            None => zero,
        },
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(traits: &[(i64, f64)]) -> Bar {
        Bar {
            owner: String::new(),
            name: "b".into(),
            id: 1,
            kind: 0,
            x: 100.0,
            y: 50.0,
            shown: true,
            elements: [Element::new(0), Element::new(1), Element::new(2), Element::new(3)],
            texts: Default::default(),
            traits: traits.iter().copied().collect(),
            meter: [0.0, 255.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            value: 0.0,
            max: 100.0,
            share: 0.0,
            item: None,
            list: None,
            presence: 1.0,
            appearing: true,
            from: [1.0, 0.0, 0.0],
            instant_hide: false,
        }
    }

    fn hides(traits: &[(i64, f64)], s: &Situation) -> Hidden {
        bar(traits).hidden_by(s, false)
    }

    const GRADUAL: Hidden = Hidden { gradual: true, instant: false };
    const INSTANT: Hidden = Hidden { gradual: false, instant: true };
    const NONE: Hidden = Hidden { gradual: false, instant: false };

    #[test]
    fn each_condition_hides_by_its_value() {
        let calm = Situation::default();
        let on = |f: fn(&mut Situation)| {
            let mut s = Situation::default();
            f(&mut s);
            s
        };
        // Nothing set hides nothing.
        assert_eq!(hides(&[], &calm), NONE);
        // 21: movement controls disabled.
        assert_eq!(hides(&[(21, 1.0)], &on(|s| s.movement_disabled = true)), GRADUAL);
        assert_eq!(hides(&[(21, 1.0)], &calm), NONE);
        // 22-24: 1 hidden while, 2 only while.
        for (k, f) in [(22, (|s: &mut Situation| s.sneaking = true) as fn(&mut Situation)), (23, |s| s.combat = true), (24, |s| s.aiming = true)] {
            assert_eq!(hides(&[(k, 1.0)], &on(f)), GRADUAL, "trait {k} 1 while it holds");
            assert_eq!(hides(&[(k, 1.0)], &calm), NONE, "trait {k} 1 otherwise");
            assert_eq!(hides(&[(k, 2.0)], &on(f)), NONE, "trait {k} 2 while it holds");
            assert_eq!(hides(&[(k, 2.0)], &calm), GRADUAL, "trait {k} 2 otherwise");
        }
        // 26: 1 hidden with the weapon out, 2 without it.
        assert_eq!(hides(&[(26, 1.0)], &on(|s| s.weapon_out = true)), GRADUAL);
        assert_eq!(hides(&[(26, 1.0)], &calm), NONE);
        assert_eq!(hides(&[(26, 2.0)], &calm), GRADUAL);
        assert_eq!(hides(&[(26, 2.0)], &on(|s| s.weapon_out = true)), NONE);
        // 25: the item test is the caller's.
        assert_eq!(bar(&[(25, 1.0)]).hidden_by(&calm, false), GRADUAL);
        assert_eq!(bar(&[(25, 1.0)]).hidden_by(&calm, true), NONE);
        // 27: 1 hidden in third person, 2 in first; at once.
        assert_eq!(hides(&[(27, 1.0)], &calm), INSTANT);
        assert_eq!(hides(&[(27, 1.0)], &on(|s| s.first_person = true)), NONE);
        assert_eq!(hides(&[(27, 2.0)], &on(|s| s.first_person = true)), INSTANT);
        // 0: 1 game mode only, 2 menu mode only; at once.
        assert_eq!(hides(&[(0, 1.0)], &on(|s| s.menu_mode = true)), INSTANT);
        assert_eq!(hides(&[(0, 1.0)], &calm), NONE);
        assert_eq!(hides(&[(0, 2.0)], &calm), INSTANT);
        // A value no page lists hides nothing.
        assert_eq!(hides(&[(22, 3.0), (27, 7.0), (21, 2.0)], &on(|s| s.sneaking = true)), NONE);
    }

    #[test]
    fn conditions_combine_as_vetoes() {
        // "Appear only when aiming in combat": 23 and 24 at 2.
        let only = [(23, 2.0), (24, 2.0)];
        let mut s = Situation { combat: true, aiming: true, ..Situation::default() };
        assert_eq!(hides(&only, &s), NONE);
        s.aiming = false;
        assert_eq!(hides(&only, &s), GRADUAL, "in combat, not aiming");
        s = Situation { aiming: true, ..Situation::default() };
        assert_eq!(hides(&only, &s), GRADUAL, "aiming outside combat");
        assert_eq!(hides(&only, &Situation::default()), GRADUAL);
        // A gradual and an instant veto together.
        let both = [(22, 1.0), (27, 2.0)];
        let s = Situation { sneaking: true, first_person: true, ..Situation::default() };
        assert_eq!(hides(&both, &s), Hidden { gradual: true, instant: true });
    }

    #[test]
    fn presence_fades_by_the_timers_and_turns_back_smoothly() {
        // Without trait 41 it goes at once.
        let mut b = bar(&[]);
        assert!(b.step_presence(GRADUAL, 0.25));
        assert_eq!(b.presence, 0.0);
        assert!(!b.appearing);
        assert!(b.step_presence(NONE, 0.25));
        assert_eq!(b.presence, 1.0);
        // 41 over 1 s, 61 over 2 s.
        let mut b = bar(&[(41, 1.0), (44, 1.0), (61, 1.0), (64, 2.0)]);
        b.step_presence(GRADUAL, 0.25);
        assert_eq!(b.presence, 0.75);
        b.step_presence(GRADUAL, 0.25);
        assert_eq!(b.presence, 0.5);
        // Turned back half way: from 0.5 up at the appearing speed.
        b.step_presence(NONE, 0.5);
        assert_eq!(b.presence, 0.75);
        assert!(b.appearing);
        b.step_presence(NONE, 5.0);
        assert_eq!(b.presence, 1.0);
        assert!(!b.step_presence(NONE, 0.25), "nothing to change");
        // An instant veto cuts a fade short.
        b.step_presence(GRADUAL, 0.25);
        b.step_presence(Hidden { gradual: true, instant: true }, 0.25);
        assert_eq!(b.presence, 0.0);
        // A timer of 0 is at once; so is a bar HudBarShowBar hides.
        let mut b = bar(&[(41, 1.0), (44, 0.0)]);
        b.step_presence(GRADUAL, 0.01);
        assert_eq!(b.presence, 0.0);
        let mut b = bar(&[(41, 1.0), (44, 5.0)]);
        b.shown = false;
        b.step_presence(GRADUAL, 0.01);
        assert_eq!(b.presence, 0.0);
    }

    #[test]
    fn a_gradual_change_moves_the_bar_without_jumps() {
        let mut b = bar(&[(41, 1.0), (42, 1.0), (43, 40.0), (44, 1.0), (61, 1.0), (62, 2.0), (63, -10.0), (64, 1.0)]);
        assert_eq!(b.place(), (100.0, 50.0));
        // Vanishing to the right: half way 20 pixels on.
        b.step_presence(GRADUAL, 0.5);
        assert_eq!(b.place(), (120.0, 50.0));
        // Turned back: from (120, 50) to its place, linear, no jump to 63's line.
        b.step_presence(NONE, 0.25);
        assert_eq!(b.presence, 0.75);
        assert_eq!(b.place(), (110.0, 50.0));
        // Turned again: from (110, 50) to 43's shift, half way at presence 0.375.
        b.step_presence(GRADUAL, 0.375);
        assert_eq!(b.place(), (125.0, 50.0));
        b.step_presence(GRADUAL, 5.0);
        assert_eq!(b.place(), (140.0, 50.0));
        // From hidden it appears from 63's shift, from above.
        b.step_presence(NONE, 0.0);
        assert_eq!(b.place(), (100.0, 40.0));
        b.step_presence(NONE, 0.75);
        assert_eq!(b.place(), (100.0, 47.5));
        b.step_presence(NONE, 1.0);
        assert_eq!(b.place(), (100.0, 50.0));
    }

    #[test]
    fn shown_again_after_an_instant_veto_is_instant() {
        let fades = [(41, 1.0), (44, 1.0), (61, 1.0), (64, 1.0)];
        // Trait 27 (or 0) hides at once and shows at once.
        let mut b = bar(&fades);
        b.step_presence(INSTANT, 0.1);
        assert_eq!(b.presence, 0.0);
        b.step_presence(NONE, 0.1);
        assert_eq!(b.presence, 1.0, "no fade in after an instant veto");
        // A gradual hide fades both ways.
        b.step_presence(GRADUAL, 0.5);
        assert_eq!(b.presence, 0.5);
        b.step_presence(GRADUAL, 1.0);
        b.step_presence(NONE, 0.25);
        assert_eq!(b.presence, 0.25, "a gradual hide fades in again");
        // The reason that hid it last decides: instant, then only a gradual one left.
        let mut b = bar(&fades);
        b.step_presence(Hidden { gradual: true, instant: true }, 0.1);
        b.step_presence(GRADUAL, 0.1);
        b.step_presence(NONE, 0.25);
        assert_eq!(b.presence, 0.25, "the gradual condition hid it last");
        // Gradual, then an instant one that outlasts it.
        let mut b = bar(&fades);
        b.step_presence(GRADUAL, 2.0);
        b.step_presence(Hidden { gradual: true, instant: true }, 0.1);
        b.step_presence(INSTANT, 0.1);
        b.step_presence(NONE, 0.1);
        assert_eq!(b.presence, 1.0, "the instant condition hid it last");
    }
}
