//! The pointer and keys on tiles (task T8; docs/spec-notes/menu-xml.md §3.2): which tile
//! is under the pointer, and the traits the original's code writes for it.
//!
//! - [`hit`]: a pure function of the tree and a pointer position in interface units.
//! - [`Pointer`]: the small state kept between frames (the tile with `mouseover`, the
//!   `clicked` pulses to end), stepped once a frame with that frame's [`Input`]; its
//!   [`Report`] says what changed, for the menu's code and the script events.
//!
//! What the documentation says, in short [documented: the GECK wiki's Target, Mouseover,
//! Clicked, Ref, Input, Hotrect, Shiftclicked pages and "Tutorial: Understanding XML"
//! §3.2 c)]: a tile with `target` true reacts to the pointer and keys; the game sets its
//! `mouseover` to 1 while it has the pointer and 0 otherwise; a click sets `clicked`; a
//! hidden tile is neither moused over nor clicked; a target hotrect extends its targeting
//! to its children; `<ref>` in a key trait (`xbuttonx`, `xup` …) sends that key to
//! another tile's `clicked` or `mouseover`. The rest is tagged where it is decided.

use crate::expr::{Arg, OpKind};
use crate::tree::{Tile, TileId, Ui};
use crate::value::Value;

/// Text measuring for text tiles without a `width` or `height`: font number, string, wrap
/// width in; interface units out (the layout's measure, `nvf-play` tiles/layout.rs).
pub type Measure<'a> = dyn Fn(f64, &str, Option<f64>) -> (f64, f64) + 'a;

/// A measure that leaves text tiles without a size at zero size (not hit).
pub fn no_measure(_: f64, _: &str, _: Option<f64>) -> (f64, f64) {
    (0.0, 0.0)
}

pub const MOUSEOVER: &str = "mouseover";
pub const CLICKED: &str = "clicked";
pub const SHIFTCLICKED: &str = "shiftclicked";

/// What `clicked` holds after a click. The two documents disagree: "set to 1 by the
/// engine for a single frame after the element is clicked by the mouse" [documented:
/// Tutorial: Understanding XML §3.2 c)] against "it will count how many times a element
/// has been clicked" [documented: Clicked]. [`CLICKED_RULE`] takes the first; a capture
/// settles it (docs/handoff/observer-checks.md, the T8 checks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickedRule {
    /// 1 on the frame of the click, back to 0 at the next step.
    OneFrame,
    /// Adds 1 a click and keeps the count.
    Count,
}

/// [hypothesis: the tutorial's reading, see [`ClickedRule`]].
pub const CLICKED_RULE: ClickedRule = ClickedRule::OneFrame;

/// The controller's key traits [documented: Input, the buttons and the four directions;
/// Hardcoded UI Shortcuts, the stick buttons `&xbuttonls;` `&xbuttonrs;`]. The arrow keys
/// press `xup` … `xright` as the D-pad does [documented: Input].
pub const PAD_TRAITS: [&str; 15] = [
    "xbuttona", "xbuttonb", "xbuttonx", "xbuttony", "xbuttonlt", "xbuttonrt", "xbuttonlb", "xbuttonrb", "xbuttonls", "xbuttonrs", "xbuttonstart", "xup",
    "xdown", "xleft", "xright",
];

/// The trait of a keyboard key on a menu: `_PCButton_<key>`, any case [observed: the
/// vanilla menu files write `_PCButton_E`, `_PCBUTTON_S`, … on the `<menu>` element only,
/// the value a tile's name; documented: LStewieAl's Tweaks, "extend vanilla XML
/// keybindings to support Numbers, F1-F12, CAPS, TAB and ALT, e.g. _PCBUTTON_1"].
pub fn pc_button_trait(key: &str) -> String {
    format!("_PCButton_{key}")
}

/// A rectangle in interface units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl Area {
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.left && x < self.left + self.width && y >= self.top && y < self.top + self.height
    }

    fn intersect(self, o: Area) -> Area {
        let left = self.left.max(o.left);
        let top = self.top.max(o.top);
        Area {
            left,
            top,
            width: ((self.left + self.width).min(o.left + o.width) - left).max(0.0),
            height: ((self.top + self.height).min(o.top + o.height) - top).max(0.0),
        }
    }
}

/// The tile under a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The topmost tile the pointer is over.
    pub under: TileId,
    /// The tile that takes the event: `under` when its own `target` is true, else the
    /// nearest tile above it whose own `target` is true (the target hotrect that made
    /// `under` a target) [hypothesis: a button's text and box are children of its target
    /// hotrect (the text_box prefab) and the button highlights over them].
    pub tile: TileId,
}

fn num(ui: &Ui, t: TileId, name: &str, default: f64) -> f64 {
    ui.tile(t).get(name).map_or(default, Value::num)
}

fn flag(ui: &Ui, t: TileId, name: &str) -> bool {
    ui.tile(t).get(name).is_some_and(Value::truthy)
}

/// The private trait of a tile whose children do not stand from its place but from where it
/// stands itself [hypothesis: the file's tabs have no `locus`, their inner rect repeats the
/// tab's place, and the original stands them at that one place, pipboy.md §5.1.1]. An
/// engine-written mark, in no game file, written by a menu's model, so nothing else changes;
/// the layout (`nvf-play`'s `tiles::layout`) reads it too, so the pointer finds what is drawn.
pub const NO_ORIGIN: &str = "_NVFNoOrigin";

/// The private trait that turns a tile's `clipwindow` on ([`mark_clip_windows`]) [design].
/// An engine-written mark, in no game file: an ordinary trait to the tree, so generic trait
/// dumps list it and a file naming it would be read too.
pub const CLIP_WINDOW_ON: &str = "_NVFClipWindow";

/// The private trait of a tile its parent's window does not crop, in the drawing and under the
/// pointer alike; windows further up still do [design: an engine-written mark, in no game file,
/// written by a menu's model; so nothing else changes]. [hypothesis] A `clipwindow` crops only what has a true `clips`: the
/// trait menu's description bar stands beside its window with none, its text in it with one, and
/// the bar's arrows show on the owner's screenshot (chargen.md §4). `clips_children` crops all
/// below it, which the other menus' lists rely on, so a menu marks the exception.
pub const OUTSIDE_CLIP: &str = "_NVFOutsideClip";

/// Whether a tile crops its children to its own box, in the drawing (`nvf_play::tiles::layout`)
/// and under the pointer ([`hit`]) alike: a true `clips` [hypothesis: menu-xml.md §3.1.1], or
/// a true `clipwindow` in a menu whose model turned it on ([`mark_clip_windows`]).
/// [hypothesis] `clipwindow` is the same rectangular crop: `list_box.xml` gives its highlight
/// box `clips` from its parent's `clipwindow`, and `tutorial_menu.xml`'s text moves inside a
/// `clipwindow` hotrect; no page says what it means (menu-xml.md §3.1.1, the observer check).
pub fn clips_children(tile: &Tile) -> bool {
    let on = |name: &str| tile.get(name).is_some_and(Value::truthy);
    on("clips") || (on("clipwindow") && on(CLIP_WINDOW_ON))
}

/// Turn `clipwindow` on for every tile under `root` that has it true, `root` included
/// [design: `clipwindow` stands in vanilla files drawn from tiles behind other switches
/// (`list_box.xml` in the message box) and in mods' tiles the default game draws, so the
/// reading reaches only a menu that asks for it].
pub fn mark_clip_windows(ui: &mut Ui, root: TileId) {
    let mut stack = vec![root];
    while let Some(t) = stack.pop() {
        if !ui.is_live(t) {
            continue;
        }
        let tile = ui.tile(t);
        let window = tile.get("clipwindow").is_some_and(Value::truthy) && !tile.get(CLIP_WINDOW_ON).is_some_and(Value::truthy);
        stack.extend(tile.children.iter().copied());
        if window {
            ui.set_trait(t, CLIP_WINDOW_ON, Value::Num(1.0));
        }
    }
}

/// Whether a tile and all above it are visible (a missing `visible` is true) [documented:
/// Tutorial, "a hidden element can't be clicked or moused over"; the layout's reading of
/// `visible`, menu-xml.md §3.1.1].
pub fn shown(ui: &Ui, t: TileId) -> bool {
    let mut at = Some(t);
    while let Some(id) = at {
        if ui.tile(id).get("visible").is_some_and(|v| !v.truthy()) {
            return false;
        }
        at = ui.tile(id).parent;
    }
    true
}

/// Whether a tile's own `target` is true.
pub fn targets(ui: &Ui, t: TileId) -> bool {
    flag(ui, t, "target")
}

/// The tile a script click (`ClickMenuTile`) acts on: `t` itself when its own `target` is
/// true, else the nearest `<hotrect>` above it whose `target` is true [documented:
/// Hotrect, "Hotrect with Target set to 1 will enforce it to its childs even if child
/// Target is set 0"; ClickMenuTile, "should work with any interactable UI tile"].
pub fn clickable_target(ui: &Ui, t: TileId) -> Option<TileId> {
    if !ui.is_live(t) {
        return None;
    }
    if targets(ui, t) {
        return Some(t);
    }
    let mut at = ui.tile(t).parent;
    while let Some(id) = at {
        let tile = ui.tile(id);
        if tile.kind == "hotrect" && targets(ui, id) {
            return Some(id);
        }
        at = tile.parent;
    }
    None
}

/// The tile itself, or the nearest above it, whose own `target` is true.
fn event_tile(ui: &Ui, t: TileId) -> Option<TileId> {
    let mut at = Some(t);
    while let Some(id) = at {
        if targets(ui, id) {
            return Some(id);
        }
        at = ui.tile(id).parent;
    }
    None
}

#[derive(Clone, Copy)]
struct Place {
    area: Area,
    depth: f64,
    clip: Option<Area>,
    /// The crop without the parent's own window, for a tile marked [`OUTSIDE_CLIP`].
    clip_above: Option<Area>,
    /// A target hotrect above makes this a target [documented: Hotrect, "Hotrect with
    /// Target set to 1 will enforce it to its childs even if child Target is set 0";
    /// hypothesis: to all below it, not only its children].
    forced: bool,
}

/// A tile's box, from its parent's place, as the layout places it (nvf-play
/// tiles/layout.rs; menu-xml.md §3.1.1): `x`/`y` added to the parent's top-left,
/// `width`/`height` its size, a text tile measured when it has none and moved by its
/// `justify`, `depth` added down the tree, `clips` cropping the children [design: the same
/// rules, so the pointer finds what is drawn; `locus` is read only where the layout reads
/// it, [`hit_candidates_by_locus`]].
fn place(ui: &Ui, t: TileId, parent: Place, measure: &Measure<'_>) -> Place {
    let tile = ui.tile(t);
    let mut area = Area {
        left: parent.area.left + num(ui, t, "x", 0.0),
        top: parent.area.top + num(ui, t, "y", 0.0),
        width: num(ui, t, "width", 0.0),
        height: num(ui, t, "height", 0.0),
    };
    if tile.kind == "text" {
        if tile.get("width").is_none() || tile.get("height").is_none() {
            let string = tile.get("string").map(Value::text).unwrap_or_default();
            let wrap = tile.get("wrapwidth").map(Value::num);
            let (w, h) = measure(num(ui, t, "font", 0.0), &string, wrap);
            area.width = num(ui, t, "width", w);
            area.height = num(ui, t, "height", h);
        }
        area.left -= match tile.get("justify").and_then(Value::sym_name).as_deref() {
            Some("center") => area.width / 2.0,
            Some("right") => area.width,
            _ => 0.0,
        };
    }
    Place {
        area,
        depth: parent.depth + num(ui, t, "depth", 0.0),
        clip: if flag(ui, t, OUTSIDE_CLIP) { parent.clip_above } else { parent.clip },
        clip_above: parent.clip_above,
        forced: parent.forced,
    }
}

/// Whether a tile of this kind can be under the pointer: not a `menu` (no box of its own)
/// and not a `rect` [documented: Target, "due how Rect behaves this trait has no affect on
/// it"; the tutorial's "(rect or image)" says otherwise, observer check].
fn hittable_kind(kind: &str) -> bool {
    !matches!(kind, "menu" | "rect")
}

/// A tile the pointer can hit, as the tree stood when it was worked out ([`hit_candidates`]):
/// everything that decides a hit, so it can be decided later on what was drawn then.
#[derive(Clone, Debug, PartialEq)]
pub struct HitCandidate {
    /// The tile under the pointer, and the tile that takes the event ([`Hit`]).
    pub under: TileId,
    pub event: Option<TileId>,
    pub area: Area,
    pub clip: Option<Area>,
    pub depth: f64,
    /// Drawing order, the tie's break.
    pub order: usize,
}

impl HitCandidate {
    /// What of it the pointer can reach, its clip taken off.
    pub fn region(&self) -> Area {
        self.clip.map_or(self.area, |c| c.intersect(self.area))
    }

    fn under_point(&self, at: (f64, f64)) -> bool {
        self.area.contains(at) && self.clip.is_none_or(|c| c.contains(at))
    }
}

/// The tile a list of one menu's candidates has on top at `at`.
pub fn top_hit(ui: &Ui, candidates: &[HitCandidate], at: (f64, f64)) -> Option<Hit> {
    let top = candidates.iter().filter(|c| c.under_point(at) && ui.is_live(c.under)).max_by(|a, b| a.depth.total_cmp(&b.depth).then(a.order.cmp(&b.order)))?;
    top.event.filter(|&t| ui.is_live(t)).map(|tile| Hit { under: top.under, tile })
}

/// Every tile of `menu` the pointer can hit as the tree stands now: live, shown, a target and
/// of a kind that takes one, with its resolved place, clip, depth and order [`hit`]'s rules.
/// Taken as a menu is drawn, and given to [`Pointer::step`] by [`Input::resolved`], a later
/// frame's click lands on what was on screen, whatever the tree has become meanwhile.
pub fn hit_candidates(ui: &Ui, menu: TileId, measure: &Measure<'_>) -> Vec<HitCandidate> {
    hit_candidates_in(ui, menu, measure, false)
}

/// [`hit_candidates`] by the `locus` rule, for a menu the layout draws by it (nvf-play
/// `tiles::layout::layout_by_locus`, the start menu's Mods page): a tile's `x` and `y`
/// count from the nearest tile above it whose `locus` is on. Without it the pointer was
/// matched against other places than the drawn ones: MCM's `MCM` rect has no `locus` and
/// an `x` of its own, so every row under it was looked for that much to the right
/// [observed: the owner's run of 2026-11-12, MCM's rows highlighting beside the pointer].
pub fn hit_candidates_by_locus(ui: &Ui, menu: TileId, measure: &Measure<'_>) -> Vec<HitCandidate> {
    hit_candidates_in(ui, menu, measure, true)
}

fn hit_candidates_in(ui: &Ui, menu: TileId, measure: &Measure<'_>, by_locus: bool) -> Vec<HitCandidate> {
    let mut out = Vec::new();
    if !ui.is_live(menu) || !shown(ui, menu) {
        return out;
    }
    let root = Place { area: Area::default(), depth: 0.0, clip: None, clip_above: None, forced: false };
    let mut order = 0;
    let mut stack = vec![(menu, root)];
    let mut child_depths: Vec<(TileId, f64)> = Vec::new();
    while let Some((id, parent)) = stack.pop() {
        let tile = ui.tile(id);
        // A hidden tile hides all below it [documented: Tutorial, `visible`].
        if !tile.live || tile.get("visible").is_some_and(|v| !v.truthy()) {
            continue;
        }
        let here = place(ui, id, parent, measure);
        order += 1;
        let own = targets(ui, id);
        if (own || here.forced) && hittable_kind(&tile.kind) {
            out.push(HitCandidate { under: id, event: event_tile(ui, id), area: here.area, clip: here.clip, depth: here.depth, order });
        }
        let mut below = here;
        below.forced |= own && tile.kind == "hotrect";
        below.clip_above = here.clip;
        if clips_children(tile) {
            below.clip = Some(below.clip.map_or(here.area, |c| c.intersect(here.area)));
        }
        if flag(ui, id, NO_ORIGIN) || (by_locus && !flag(ui, id, "locus")) {
            below.area.left = parent.area.left;
            below.area.top = parent.area.top;
        }
        match tile.children.as_slice() {
            [] => {}
            &[only] => stack.push((only, below)),
            children => {
                child_depths.clear();
                let mut sorted = true;
                let mut prev = f64::NEG_INFINITY;
                for &c in children {
                    let d = num(ui, c, "depth", 0.0);
                    if d.total_cmp(&prev).is_lt() {
                        sorted = false;
                    }
                    prev = d;
                    child_depths.push((c, d));
                }
                if !sorted {
                    child_depths.sort_by(|a, b| a.1.total_cmp(&b.1));
                }
                stack.extend(child_depths.iter().rev().map(|&(c, _)| (c, below)));
            }
        }
    }
    out
}

/// The tile under the pointer at `at` (interface units) among `menus`, back to front.
///
/// - Only live, shown tiles that are targets (their own `target`, or below a target
///   hotrect) and not a `rect` or `menu` [documented: see [`hittable_kind`], [`shown`]].
/// - `alpha` is not read [hypothesis: every vanilla text button is a target hotrect with
///   `alpha` 0 (the text_box prefab), and the save list's rows too, so alpha 0 cannot
///   stop the pointer] [observed: the vanilla prefabs].
/// - A later menu in `menus` is over an earlier one whatever their depths [hypothesis: the
///   menu opened last takes the pointer].
/// - In one menu the highest `depth` (added down the tree) wins, a tie going to the tile
///   drawn later [documented: Tutorial, "the element with the higher value will be drawn
///   on top"; hypothesis: the pointer takes the tile drawn on top; observer check].
pub fn hit(ui: &Ui, menus: &[TileId], at: (f64, f64), measure: &Measure<'_>) -> Option<Hit> {
    for &menu in menus.iter().rev() {
        let found = hit_candidates(ui, menu, measure);
        if found.iter().any(|c| c.under_point(at) && ui.is_live(c.under)) {
            return top_hit(ui, &found, at);
        }
    }
    None
}

/// The first tile of an `id` in a menu, in file order [documented: ClickMenuButton,
/// `"RecipeMenu#2"` names the tile whose `<id>` is 2; hypothesis: the first when two
/// share one].
pub fn tile_with_id(ui: &Ui, menu: TileId, id: f64) -> Option<TileId> {
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        if !ui.tile(t).live {
            continue;
        }
        if t != menu && ui.tile(t).get("id").is_some_and(|v| v.num() == id) {
            return Some(t);
        }
        stack.extend(ui.tile(t).children.iter().rev().copied());
    }
    None
}

/// A tile by a path or `Menu#id` [documented: ClickMenuButton, SetOnMenuClickEventHandler].
pub fn find(ui: &Ui, path: &str) -> Option<TileId> {
    if let Some((menu, id)) = path.split_once('#') {
        let id: f64 = id.trim().parse().ok()?;
        return tile_with_id(ui, ui.menu(menu.trim())?, id);
    }
    ui.find_tile(path)
}

/// How a click came.
#[derive(Clone, Debug, PartialEq)]
pub enum By {
    Pointer,
    /// A keyboard key's `_PCButton_*`, or a controller trait's `<ref>`.
    Key(String),
    /// ClickMenuButton, ClickMenuTile.
    Script,
}

/// A click on a tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Click {
    pub tile: TileId,
    /// Its `id`, for the menu's code [documented: Tutorial, "if the element has an <id>
    /// property, clicking on it will set it to 'clicked'"; hypothesis: `clicked` is written
    /// on any target, as the Clicked page says, and the id is what the menu's code reads].
    pub id: Option<f64>,
    pub by: By,
    /// [`Pointer::stamp`] when the click was made, a script's when it was queued: what its
    /// maker says the screen stood for then, so a click reported later can be told from one
    /// on what has replaced it [design].
    pub stamp: u64,
}

/// One frame's input for the menus that take it.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// The pointer in interface units; `None` when there is none (no cursor, or outside
    /// the view): the highlight then holds only when a key set it.
    pub at: Option<(f64, f64)>,
    /// The left mouse button went down this frame [hypothesis: a click is the press, not
    /// the release].
    pub press: bool,
    /// Shift is held: a click also pulses `shiftclicked` [documented: Shiftclicked].
    pub shift: bool,
    /// Keyboard keys pressed this frame, by the name after `_PCButton_` (`"E"`).
    pub keys: Vec<String>,
    /// Controller buttons and arrow keys pressed this frame, by trait name ([`PAD_TRAITS`]).
    pub pads: Vec<String>,
    /// Decide the pointer's tile on these ([`hit_candidates`]) instead of the live tree.
    pub resolved: Option<Vec<HitCandidate>>,
}

/// What a step changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The highlight moved: (from, to).
    pub over: Option<(Option<TileId>, Option<TileId>)>,
    /// A key's `<ref … trait="mouseover"/>` moved it, not the pointer, wherever it went: back
    /// onto the tile under the pointer too.
    pub over_by_key: bool,
    /// Every click since the last step: the scripts' (ClickMenuButton, ClickMenuTile)
    /// first, then this frame's.
    pub clicks: Vec<Click>,
    /// Keys whose trait named nothing this code follows, one line each.
    pub unfollowed: Vec<String>,
}

/// The pointer's state between frames.
#[derive(Debug, Default)]
pub struct Pointer {
    /// Where the pointer was last step.
    pub at: Option<(f64, f64)>,
    /// The tile whose `mouseover` is 1 (GetActiveUIComponentName's tile).
    pub over: Option<TileId>,
    /// A key moved the highlight (`<ref … trait="mouseover"/>`): it holds until the
    /// pointer moves [hypothesis], or its tile is hidden or stops being a target [design].
    keyed: bool,
    /// Tiles whose `clicked` (and `shiftclicked`) go back to 0, with the steps each still
    /// holds: 0 ends at the next step.
    pulsed: Vec<(TileId, u8)>,
    /// How many steps a pointer's click holds `clicked` at 1; 0 is 1, the default
    /// ([`ClickedRule::OneFrame`]). The engine sets 2 under the pause menu's Mods page:
    /// MCM's scripts read a click in two frames running. They see the list's `_index`,
    /// leave the frame ("Skip frame to let UI set variables"), and read the clicked row's
    /// `_modID` in the next; then they mask the click for one frame (`_UnClick`), so a
    /// third frame would be a second click [observed: MCM 1.5's script source and menu
    /// files; hypothesis: the original's `clicked` reads 1 in exactly two MenuMode
    /// frames; with one, a click on MCM's rows did nothing, the owner's run of
    /// 2026-11-12]. A script's click stays one step.
    pub click_steps: u8,
    /// The scripts' clicks since the last step.
    queued: Vec<Click>,
    /// Copied to each click as it is made: the engine's to set ([`Click::stamp`]).
    pub stamp: u64,
}

impl Pointer {
    /// The highlighted tile, if it is still in the tree.
    pub fn active(&self, ui: &Ui) -> Option<TileId> {
        self.over.filter(|&t| ui.is_live(t))
    }

    /// Whether a step has work left over from the last: `clicked` pulses to end or the
    /// scripts' clicks to report. The engine steps for these even with no menu taking
    /// input, so a script's click lasts one frame and fires its event once [design: the
    /// maintainer's review of #957].
    pub fn pending(&self) -> bool {
        !self.pulsed.is_empty() || !self.queued.is_empty()
    }

    /// One frame: end last frame's `clicked` pulses, follow the pointer, click, press the
    /// keys. `menus` are the menus that take input, back to front; the last one takes the
    /// keys [hypothesis: the menu on top].
    pub fn step(&mut self, ui: &mut Ui, menus: &[TileId], input: &Input, measure: &Measure<'_>) -> Report {
        let mut report = Report::default();
        self.end_pulses(ui);
        report.clicks.append(&mut self.queued);
        if self.over.is_some_and(|t| !ui.is_live(t)) {
            self.over = None;
            self.keyed = false;
        }
        let moved = input.at != self.at;
        self.at = input.at;
        // A key-moved highlight goes when the pointer moves, or when its tile is hidden or
        // stops being a target (it could then be clicked by a press): the pointer's tile
        // takes it again [design: the maintainer's review of #957].
        if moved || self.over.is_some_and(|t| !Self::takes_keys(ui, t)) {
            self.keyed = false;
        }
        if !self.keyed {
            let new = input
                .at
                .and_then(|at| match &input.resolved {
                    Some(candidates) => top_hit(ui, candidates, at),
                    None => hit(ui, menus, at, measure),
                })
                .map(|h| h.tile);
            self.highlight(ui, new, &mut report);
        }
        if input.press {
            if let Some(t) = self.over {
                report.clicks.push(self.click(ui, t, input.shift, By::Pointer));
            }
        }
        let top = menus.iter().rev().copied().find(|&m| ui.is_live(m));
        for key in &input.keys {
            if let Some(menu) = top {
                self.pc_key(ui, menu, key, input.shift, &mut report);
            }
        }
        for pad in &input.pads {
            if let Some(menu) = top {
                self.pad(ui, menu, pad, input.shift, &mut report);
            }
        }
        report
    }

    /// A click as a script makes it (ClickMenuButton, ClickMenuTile): `clicked` set now,
    /// so the same frame's scripts read it; reported at the next [`Pointer::step`].
    pub fn script_click(&mut self, ui: &mut Ui, tile: TileId) -> Click {
        let c = self.click(ui, tile, false, By::Script);
        self.queued.push(c.clone());
        c
    }

    fn click(&mut self, ui: &mut Ui, t: TileId, shift: bool, by: By) -> Click {
        match CLICKED_RULE {
            ClickedRule::OneFrame => ui.set_trait(t, CLICKED, Value::Num(1.0)),
            ClickedRule::Count => {
                let n = num(ui, t, CLICKED, 0.0);
                ui.set_trait(t, CLICKED, Value::Num(n + 1.0));
            }
        }
        if shift {
            ui.set_trait(t, SHIFTCLICKED, Value::Num(1.0));
        }
        let more = if by == By::Pointer { self.click_steps.saturating_sub(1) } else { 0 };
        match self.pulsed.iter_mut().find(|p| p.0 == t) {
            Some(p) => p.1 = more,
            None => self.pulsed.push((t, more)),
        }
        Click { tile: t, id: ui.tile(t).get("id").map(Value::num), by, stamp: self.stamp }
    }

    fn end_pulses(&mut self, ui: &mut Ui) {
        for (t, more) in std::mem::take(&mut self.pulsed) {
            if !ui.is_live(t) {
                continue;
            }
            if more > 0 {
                self.pulsed.push((t, more - 1));
                continue;
            }
            if CLICKED_RULE == ClickedRule::OneFrame && ui.tile(t).get(CLICKED).is_some() {
                ui.set_trait(t, CLICKED, Value::Num(0.0));
            }
            if ui.tile(t).get(SHIFTCLICKED).is_some() {
                ui.set_trait(t, SHIFTCLICKED, Value::Num(0.0));
            }
        }
    }

    /// Moves `mouseover`: 0 on the tile that had it, 1 on the new one [documented:
    /// Tutorial, "set to 1 … when the element has mouse focus. It is set to 0 otherwise";
    /// hypothesis: only on the tile taking the event, not the tiles above it].
    fn highlight(&mut self, ui: &mut Ui, new: Option<TileId>, report: &mut Report) {
        if new == self.over {
            return;
        }
        if let Some(old) = self.over.filter(|&t| ui.is_live(t)) {
            ui.set_trait(old, MOUSEOVER, Value::Num(0.0));
        }
        if let Some(t) = new {
            ui.set_trait(t, MOUSEOVER, Value::Num(1.0));
        }
        let from = report.over.map_or(self.over, |(from, _)| from);
        report.over = Some((from, new));
        self.over = new;
    }

    /// Whether a key may act on this tile: shown and a target [hypothesis: as the pointer;
    /// a disabled button's `target` is 0 (the button_basic prefab copies `_enabled`)].
    fn takes_keys(ui: &Ui, t: TileId) -> bool {
        ui.is_live(t) && shown(ui, t) && targets(ui, t)
    }

    /// `_PCButton_<key>` on the menu names the tile the key clicks [observed: the vanilla
    /// menu files, e.g. `_PCButton_E` naming the exit button; documented: the menus' own
    /// key hints, docs/spec-notes/lockpicking.md §2 "E is Exit"]. The name is looked for as
    /// an operator's unbracketed `src` is: the first tile of that name in the menu.
    fn pc_key(&mut self, ui: &mut Ui, menu: TileId, key: &str, shift: bool, report: &mut Report) {
        let Some(name) = ui.tile(menu).get(&pc_button_trait(key)).map(Value::text) else { return };
        let name = name.trim().to_string();
        match ui.resolve(menu, &crate::expr::Src::Named(name.clone())) {
            Some(t) if t != menu && Self::takes_keys(ui, t) => {
                let c = self.click(ui, t, shift, By::Key(key.to_string()));
                report.clicks.push(c);
            }
            Some(_) => {}
            None => report.unfollowed.push(format!("{}: {} names no tile", pc_button_trait(key), name)),
        }
    }

    /// A controller trait: the highlighted tile's, else the menu's [hypothesis: the
    /// vanilla files put `clicked` refs on the `<menu>` and `mouseover` refs on list
    /// items; documented: Input, "once mouseover becomes true on this tile, clicking one of
    /// the button will execute the function inside it"]. Its `<ref src=… trait=…/>` is
    /// followed: `clicked` clicks that tile, `mouseover` highlights it [documented: Ref,
    /// Input]; another trait is reported, not followed [design]. When the highlighted
    /// tile's ref does nothing (it names no tile, a tile hidden or not a target, or a trait
    /// not followed), the menu's is tried [hypothesis: "else the menu's" read as "when
    /// the tile's does nothing"; the maintainer's review of #957].
    fn pad(&mut self, ui: &mut Ui, menu: TileId, name: &str, shift: bool, report: &mut Report) {
        let first = self.active(ui).filter(|&t| t != menu);
        for on in [first, Some(menu)].into_iter().flatten() {
            let Some((src, trait_name)) = ref_of(ui, on, name) else { continue };
            let Some(t) = ui.resolve(on, &src) else {
                report.unfollowed.push(format!("{name}: its ref names no tile"));
                continue;
            };
            if !Self::takes_keys(ui, t) {
                continue;
            }
            if trait_name.eq_ignore_ascii_case(CLICKED) {
                let c = self.click(ui, t, shift, By::Key(name.to_string()));
                report.clicks.push(c);
                return;
            } else if trait_name.eq_ignore_ascii_case(MOUSEOVER) {
                let before = self.over;
                self.highlight(ui, Some(t), report);
                self.keyed = true;
                if self.over != before {
                    report.over_by_key = true;
                }
                return;
            }
            report.unfollowed.push(format!("{name}: ref to {trait_name} not followed"));
        }
    }
}

/// The first `<ref>` operator of a tile's trait: its source and trait name.
fn ref_of(ui: &Ui, t: TileId, name: &str) -> Option<(crate::expr::Src, String)> {
    let tile = ui.tile(t);
    let ops = tile.traits.get(tile.trait_index(name)?)?.expr.as_ref()?;
    ops.iter().find_map(|op| match (&op.kind, &op.arg) {
        (OpKind::Ref, Arg::Ref { src, name }) => Some((src.clone(), name.clone())),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::MemSource;

    /// A synthetic menu: two overlapping buttons (hotrects with a text child), a hidden
    /// button, a plain image target, an untargeted rect over everything, the keys.
    const MENU: &str = r#"<menu name="TestMenu">
  <class> &QuantityMenu; </class>
  <xbuttona> <ref src="OkButton" trait="clicked"/> </xbuttona>
  <xbuttonb> <ref src="Nowhere" trait="clicked"/> </xbuttonb>
  <xbuttonx> <ref src="OkButton" trait="_poke"/> </xbuttonx>
  <_PCButton_E> CancelButton </_PCButton_E>
  <_PCButton_H> HiddenButton </_PCButton_H>
  <_PCBUTTON_S> OkButton </_PCBUTTON_S>
  <rect name="Main">
    <x> 100 </x> <y> 100 </y> <width> 800 </width> <height> 600 </height>
    <hotrect name="OkButton">
      <id> 1 </id> <target> &true; </target> <alpha> 0 </alpha>
      <x> 10 </x> <y> 10 </y> <width> 200 </width> <height> 50 </height> <depth> 1 </depth>
      <text name="label"> <x> 5 </x> <y> 5 </y> <width> 100 </width> <height> 20 </height> <depth> 2 </depth> </text>
      <xdown> <ref src="sibling(CancelButton)" trait="mouseover"/> </xdown>
      <_times> <copy src="me()" trait="_times"/> <add src="me()" trait="clicked"/> </_times>
    </hotrect>
    <hotrect name="CancelButton">
      <id> 2 </id> <target> &true; </target>
      <x> 110 </x> <y> 30 </y> <width> 200 </width> <height> 50 </height> <depth> 5 </depth>
      <xup> <ref src="sibling(OkButton)" trait="mouseover"/> </xup>
      <xbuttona> <ref src="sibling(HiddenButton)" trait="clicked"/> </xbuttona>
    </hotrect>
    <hotrect name="HiddenButton">
      <id> 3 </id> <target> &true; </target> <visible> &false; </visible>
      <x> 400 </x> <y> 10 </y> <width> 100 </width> <height> 100 </height> <depth> 9 </depth>
    </hotrect>
    <image name="Arrow"> <target> &true; </target> <x> 600 </x> <y> 10 </y> <width> 20 </width> <height> 20 </height> </image>
    <image name="Plain"> <x> 600 </x> <y> 200 </y> <width> 20 </width> <height> 20 </height> <depth> 50 </depth> </image>
    <rect name="Cover"> <target> &true; </target> <x> 0 </x> <y> 0 </y> <width> 800 </width> <height> 600 </height> <depth> 99 </depth> </rect>
  </rect>
</menu>"#;

    #[test]
    fn a_tile_marked_outside_a_window_is_hit_there() {
        let src = MemSource::default().with(
            "w.xml",
            r#"<menu name="W"><hotrect name="Box"><width>100</width><height>100</height><clips>&true;</clips>
              <image name="Beside"><target>&true;</target><x>100</x><width>20</width><height>20</height></image>
              <image name="In"><target>&true;</target><width>20</width><height>20</height></image>
            </hotrect></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "w.xml").unwrap();
        ui.update();
        assert!(hit(&ui, &[menu], (110.0, 10.0), &no_measure).is_none(), "cropped by the window");
        let beside = ui.find_tile("W/Box/Beside").unwrap();
        ui.set_trait(beside, OUTSIDE_CLIP, Value::Num(1.0));
        ui.update();
        assert_eq!(hit(&ui, &[menu], (110.0, 10.0), &no_measure).map(|h| h.tile), Some(beside));
        let inside = ui.find_tile("W/Box/In").unwrap();
        assert_eq!(hit(&ui, &[menu], (10.0, 10.0), &no_measure).map(|h| h.tile), Some(inside));
        // A window above that one still crops it.
        let src = MemSource::default().with(
            "o.xml",
            r#"<menu name="O"><hotrect name="Outer"><width>105</width><height>100</height><clips>&true;</clips>
              <hotrect name="Box"><width>100</width><height>100</height><clips>&true;</clips>
              <image name="Beside"><target>&true;</target><x>100</x><width>20</width><height>20</height></image></hotrect></hotrect></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "o.xml").unwrap();
        ui.update();
        let beside = ui.find_tile("O/Outer/Box/Beside").unwrap();
        ui.set_trait(beside, OUTSIDE_CLIP, Value::Num(1.0));
        ui.update();
        assert_eq!(hit(&ui, &[menu], (102.0, 10.0), &no_measure).map(|h| h.tile), Some(beside));
        assert!(hit(&ui, &[menu], (110.0, 10.0), &no_measure).is_none(), "cropped by the outer window");
    }

    fn tree() -> (Ui, TileId) {
        let src = MemSource::default().with("menus\\test.xml", MENU);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "menus\\test.xml").unwrap();
        ui.update();
        (ui, menu)
    }

    fn id(ui: &Ui, path: &str) -> TileId {
        ui.find_tile(path).unwrap_or_else(|| panic!("{path}"))
    }

    fn at(x: f64, y: f64) -> Input {
        Input { at: Some((x, y)), ..Input::default() }
    }

    #[test]
    fn the_higher_depth_wins_where_hotrects_overlap() {
        let (ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let cancel = id(&ui, "TestMenu/Main/CancelButton");
        // Ok alone at (120, 115); both at (220, 140): Cancel's depth 5 over Ok's 1.
        assert_eq!(hit(&ui, &[menu], (120.0, 115.0), &no_measure).map(|h| h.tile), Some(ok));
        assert_eq!(hit(&ui, &[menu], (220.0, 140.0), &no_measure).map(|h| h.tile), Some(cancel));
        // Outside both, and over a plain image (no target) or the targeted rect: nothing.
        assert_eq!(hit(&ui, &[menu], (50.0, 50.0), &no_measure), None);
        assert_eq!(hit(&ui, &[menu], (705.0, 305.0), &no_measure), None);
        // The image target, at alpha 255 by default; the hotrect at alpha 0 above was hit.
        assert_eq!(hit(&ui, &[menu], (705.0, 115.0), &no_measure).map(|h| h.tile), Some(id(&ui, "TestMenu/Main/Arrow")));
    }

    #[test]
    fn a_target_hotrects_child_takes_the_pointer_for_it() {
        let (ui, menu) = tree();
        // The label (depth 1 + 2) is over the button; it has no target of its own, the
        // hotrect's makes it one, and the event goes to the hotrect.
        let h = hit(&ui, &[menu], (120.0, 120.0), &no_measure).unwrap();
        assert_eq!(h.under, id(&ui, "TestMenu/Main/OkButton/label"));
        assert_eq!(h.tile, id(&ui, "TestMenu/Main/OkButton"));
    }

    #[test]
    fn a_hidden_tile_is_not_hit() {
        let (mut ui, menu) = tree();
        assert_eq!(hit(&ui, &[menu], (550.0, 150.0), &no_measure), None);
        assert!(ui.set_float("TestMenu/Main/HiddenButton/visible", 1.0));
        assert_eq!(hit(&ui, &[menu], (550.0, 150.0), &no_measure).map(|h| h.tile), Some(id(&ui, "TestMenu/Main/HiddenButton")));
        // A hidden parent hides its children too.
        let main = id(&ui, "TestMenu/Main");
        ui.set_trait(main, "visible", Value::Sym("false".into()));
        assert_eq!(hit(&ui, &[menu], (550.0, 150.0), &no_measure), None);
    }

    /// A target inside a `clipwindow` hotrect, reaching past the window's box.
    fn clip_window(value: &str) -> (Ui, TileId) {
        let xml = format!(
            r#"<menu name="ClipMenu">
  <hotrect name="Window"><x>100</x><y>100</y><width>200</width><height>100</height><clipwindow>{value}</clipwindow>
    <hotrect name="Long"><id>1</id><target>&true;</target><y>-50</y><width>200</width><height>400</height></hotrect>
  </hotrect></menu>"#
        );
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("clip.xml", &xml), "clip.xml").unwrap();
        ui.update();
        let menu = ui.menu("ClipMenu").unwrap();
        (ui, menu)
    }

    #[test]
    fn clipwindow_excludes_clipped_targets() {
        let (mut ui, menu) = clip_window("&true;");
        let long = id(&ui, "ClipMenu/Window/Long");
        let under = |ui: &Ui, at| hit(ui, &[menu], at, &no_measure).map(|h| h.tile);
        // Off, as in every menu that did not ask for it: the child takes the pointer outside
        // the window as before.
        assert_eq!(under(&ui, (150.0, 150.0)), Some(long));
        assert_eq!(under(&ui, (150.0, 300.0)), Some(long));
        // On: only inside the window's box, top and bottom.
        mark_clip_windows(&mut ui, menu);
        ui.update();
        assert_eq!(under(&ui, (150.0, 150.0)), Some(long));
        assert_eq!(under(&ui, (150.0, 300.0)), None, "below the window");
        assert_eq!(under(&ui, (150.0, 75.0)), None, "above the window");
    }

    #[test]
    fn clipwindow_is_read_only_when_true_and_marked() {
        // A false `clipwindow` crops nothing and gets no mark.
        let (mut ui, menu) = clip_window("&false;");
        let window = id(&ui, "ClipMenu/Window");
        mark_clip_windows(&mut ui, menu);
        assert!(ui.tile(window).get(CLIP_WINDOW_ON).is_none());
        assert!(!clips_children(ui.tile(window)));
        // A true one crops only once marked; `clips` crops by itself.
        let (mut ui, menu) = clip_window("&true;");
        let window = id(&ui, "ClipMenu/Window");
        assert!(!clips_children(ui.tile(window)));
        mark_clip_windows(&mut ui, menu);
        assert!(clips_children(ui.tile(window)));
        let long = id(&ui, "ClipMenu/Window/Long");
        assert!(!clips_children(ui.tile(long)));
        ui.set_trait(long, "clips", Value::Num(1.0));
        assert!(clips_children(ui.tile(long)));
    }

    #[test]
    fn a_later_menu_is_over_an_earlier_one() {
        let (mut ui, menu) = tree();
        let src = MemSource::default().with(
            "menus\\top.xml",
            r#"<menu name="TopMenu"> <image name="Big"> <target> 1 </target> <width> 2000 </width> <height> 2000 </height> </image> </menu>"#,
        );
        let top = ui.load_menu(&src, "menus\\top.xml").unwrap();
        ui.update();
        assert_eq!(hit(&ui, &[menu, top], (120.0, 115.0), &no_measure).map(|h| h.tile), Some(id(&ui, "TopMenu/Big")));
        assert_eq!(hit(&ui, &[top, menu], (120.0, 115.0), &no_measure).map(|h| h.tile), Some(id(&ui, "TestMenu/Main/OkButton")));
    }

    #[test]
    fn mouseover_follows_the_pointer_and_leaves() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let cancel = id(&ui, "TestMenu/Main/CancelButton");
        let mut p = Pointer::default();
        let r = p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert_eq!(r.over, Some((None, Some(ok))));
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/mouseover"), Some(1.0));
        // The parent rect and the child label get none (see production `Pointer::step` above).
        assert_eq!(ui.get_float("TestMenu/Main/mouseover"), None);
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/label/mouseover"), None);
        // A still pointer: no change, nothing reported.
        assert_eq!(p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure).over, None);
        let r = p.step(&mut ui, &[menu], &at(220.0, 140.0), &no_measure);
        assert_eq!(r.over, Some((Some(ok), Some(cancel))));
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/mouseover"), Some(0.0));
        assert_eq!(ui.get_float("TestMenu/Main/CancelButton/mouseover"), Some(1.0));
        let r = p.step(&mut ui, &[menu], &at(10.0, 10.0), &no_measure);
        assert_eq!(r.over, Some((Some(cancel), None)));
        assert_eq!(ui.get_float("TestMenu/Main/CancelButton/mouseover"), Some(0.0));
        assert_eq!(p.over, None);
        // No pointer at all: nothing highlighted, nothing to change.
        let r = p.step(&mut ui, &[menu], &Input::default(), &no_measure);
        assert_eq!(r.over, None);
    }

    #[test]
    fn clicked_lasts_one_frame() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let mut p = Pointer::default();
        p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        let r = p.step(&mut ui, &[menu], &Input { press: true, ..at(120.0, 115.0) }, &no_measure);
        assert_eq!(r.clicks, [Click { tile: ok, id: Some(1.0), by: By::Pointer, stamp: 0 }]);
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/clicked"), Some(1.0));
        ui.update();
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/_times"), Some(1.0));
        // The next frame, the button still held or not: back to 0 (see production `Pointer::step` above).
        let r = p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert!(r.clicks.is_empty());
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/clicked"), Some(0.0));
        ui.update();
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/_times"), Some(1.0), "the count's own add of 0 keeps it");
        // No tile under the pointer: a press clicks nothing.
        let r = p.step(&mut ui, &[menu], &Input { press: true, ..at(10.0, 10.0) }, &no_measure);
        assert!(r.clicks.is_empty());
    }

    /// By the `locus` rule a tile without `locus` moves nothing below it: MCM's window
    /// rect has an `x` and no `locus`, its list has `locus` and the same `x`, and the rows
    /// stand at the list's place, not at twice the window's.
    #[test]
    fn the_locus_rule_places_the_pointers_tiles_as_the_layout_does() {
        let src = MemSource::default().with(
            "menus\\l.xml",
            r#"<menu name="L"> <rect name="Window"> <x> 300 </x> <y> 100 </y>
                 <rect name="List"> <locus> &true; </locus> <x> <copy src="parent()" trait="x"/> </x> <y> <copy src="parent()" trait="y"/> </y>
                   <hotrect name="Row"> <target> &true; </target> <x> 10 </x> <y> 20 </y> <width> 100 </width> <height> 30 </height> </hotrect>
                 </rect> </rect> </menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "menus\\l.xml").unwrap();
        ui.update();
        let row = id(&ui, "L/Window/List/Row");
        let by_locus = hit_candidates_by_locus(&ui, menu, &no_measure);
        assert_eq!(by_locus.iter().find(|c| c.under == row).map(|c| (c.area.left, c.area.top)), Some((310.0, 120.0)));
        // Every parent's place added up, the rule elsewhere: twice the window's.
        let summed = hit_candidates(&ui, menu, &no_measure);
        assert_eq!(summed.iter().find(|c| c.under == row).map(|c| (c.area.left, c.area.top)), Some((610.0, 220.0)));
        // The pointer follows the candidates it is given.
        let mut p = Pointer::default();
        let r = p.step(&mut ui, &[menu], &Input { at: Some((320.0, 130.0)), resolved: Some(by_locus), ..Input::default() }, &no_measure);
        assert_eq!(r.over, Some((None, Some(row))));
    }
    /// Under MCM's page the engine asks for two steps (`Pointer::click_steps`): the click
    /// is reported once, `clicked` reads 1 in the step of the press and the next, and 0
    /// after; a script's click stays one step.
    #[test]
    fn a_pointers_click_can_hold_two_steps() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let clicked = |ui: &Ui| ui.get_float("TestMenu/Main/OkButton/clicked");
        let mut p = Pointer { click_steps: 2, ..Pointer::default() };
        p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        let r = p.step(&mut ui, &[menu], &Input { press: true, ..at(120.0, 115.0) }, &no_measure);
        assert_eq!(r.clicks.len(), 1);
        assert_eq!(clicked(&ui), Some(1.0));
        let r = p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert!(r.clicks.is_empty(), "held, not clicked again");
        assert_eq!(clicked(&ui), Some(1.0));
        assert!(p.pending());
        p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert_eq!(clicked(&ui), Some(0.0));
        assert!(!p.pending());
        p.script_click(&mut ui, ok);
        assert_eq!(clicked(&ui), Some(1.0));
        p.step(&mut ui, &[menu], &at(10.0, 10.0), &no_measure);
        assert_eq!(clicked(&ui), Some(0.0), "a script's click is one step");
    }

    #[test]
    fn shift_click_pulses_shiftclicked() {
        let (mut ui, menu) = tree();
        let mut p = Pointer::default();
        p.step(&mut ui, &[menu], &Input { press: true, shift: true, ..at(120.0, 115.0) }, &no_measure);
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/shiftclicked"), Some(1.0));
        p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/shiftclicked"), Some(0.0));
    }

    #[test]
    fn pc_keys_click_the_tile_the_menu_names() {
        let (mut ui, menu) = tree();
        let cancel = id(&ui, "TestMenu/Main/CancelButton");
        let mut p = Pointer::default();
        let r = p.step(&mut ui, &[menu], &Input { keys: vec!["e".into()], ..Input::default() }, &no_measure);
        assert_eq!(r.clicks, [Click { tile: cancel, id: Some(2.0), by: By::Key("e".into()), stamp: 0 }]);
        assert_eq!(ui.get_float("TestMenu/Main/CancelButton/clicked"), Some(1.0));
        // A hidden button takes no key; a key no trait names does nothing.
        let r = p.step(&mut ui, &[menu], &Input { keys: vec!["H".into(), "Q".into()], ..Input::default() }, &no_measure);
        assert!(r.clicks.is_empty() && r.unfollowed.is_empty(), "{r:?}");
        assert_eq!(ui.get_float("TestMenu/Main/CancelButton/clicked"), Some(0.0));
        // The trait's name is compared without case: `_PCBUTTON_S`, as some vanilla files
        // write it [observed: menu-xml.md §3.2], answers an S press.
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let r = p.step(&mut ui, &[menu], &Input { keys: vec!["S".into()], ..Input::default() }, &no_measure);
        assert_eq!(r.clicks, [Click { tile: ok, id: Some(1.0), by: By::Key("S".into()), stamp: 0 }]);
    }

    /// A marked tile's children stand from where it stands, not from its place.
    #[test]
    fn a_tile_marked_no_origin_gives_its_children_no_place() {
        let src = crate::MemSource::default().with(
            "n.xml",
            r#"<menu name="N"><rect name="Line"><x>11</x><y>23</y>
              <hotrect name="Tab"><target>&true;</target><x>101</x><y>53</y><width>30</width><height>10</height>
                <image name="Inner"><x>101</x><y>53</y><width>30</width><height>10</height></image></hotrect></rect></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "n.xml").unwrap();
        ui.update();
        let under = |ui: &Ui, at| hit(ui, &[menu], at, &no_measure).map(|h| ui.tile(h.under).name.clone());
        // Unmarked, the child stands from the tab, twice over.
        assert_eq!(under(&ui, (218.0, 134.0)).as_deref(), Some("Inner"));
        let tab = ui.find_tile("N/Line/Tab").unwrap();
        ui.set_trait(tab, NO_ORIGIN, Value::Num(1.0));
        ui.update();
        assert_eq!(under(&ui, (218.0, 134.0)), None);
        assert_eq!(under(&ui, (117.0, 81.0)).as_deref(), Some("Inner"), "at the tab's one place");
    }

    #[test]
    fn pad_refs_are_followed() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let cancel = id(&ui, "TestMenu/Main/CancelButton");
        let mut p = Pointer::default();
        // The menu's xbuttona clicks OkButton through its ref.
        let r = p.step(&mut ui, &[menu], &Input { pads: vec!["xbuttona".into()], ..Input::default() }, &no_measure);
        assert_eq!(r.clicks, [Click { tile: ok, id: Some(1.0), by: By::Key("xbuttona".into()), stamp: 0 }]);
        // A ref to no tile, and one to a trait not followed, are reported.
        let r = p.step(&mut ui, &[menu], &Input { pads: vec!["xbuttonb".into(), "xbuttonx".into()], ..Input::default() }, &no_measure);
        assert!(r.clicks.is_empty());
        assert_eq!(r.unfollowed.len(), 2, "{r:?}");
        // With Ok highlighted, its own xdown moves the highlight to Cancel, and it holds
        // while the pointer stays where it is.
        p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        let r = p.step(&mut ui, &[menu], &Input { pads: vec!["xdown".into()], ..at(120.0, 115.0) }, &no_measure);
        assert_eq!(r.over, Some((Some(ok), Some(cancel))));
        assert!(r.over_by_key);
        assert_eq!(ui.get_float("TestMenu/Main/CancelButton/mouseover"), Some(1.0));
        assert_eq!(p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure).over, None);
        assert_eq!(p.over, Some(cancel));
        // Cancel's xup goes back; then the pointer moving takes the highlight again.
        let r = p.step(&mut ui, &[menu], &Input { pads: vec!["xup".into()], ..at(120.0, 115.0) }, &no_measure);
        assert!(r.over_by_key);
        assert_eq!(p.over, Some(ok));
        let r = p.step(&mut ui, &[menu], &at(220.0, 140.0), &no_measure);
        assert!(!r.over_by_key, "pure pointer highlight move is not a key navigation");
        assert_eq!(p.over, Some(cancel));
        // A same-frame pointer move from Cancel onto Ok paired with Ok's xdown
        // navigates Ok -> Cancel by pad, so over_by_key is true.
        let r = p.step(&mut ui, &[menu], &Input { pads: vec!["xdown".into()], ..at(120.0, 115.0) }, &no_measure);
        assert_eq!(r.over, Some((Some(cancel), Some(cancel))));
        assert!(r.over_by_key, "xdown moved Ok -> Cancel after pointer moved Cancel -> Ok");
    }

    #[test]
    fn a_pad_ref_that_cannot_act_falls_through_to_the_menus() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let cancel = id(&ui, "TestMenu/Main/CancelButton");
        let hidden = id(&ui, "TestMenu/Main/HiddenButton");
        let a = |p: &mut Pointer, ui: &mut Ui| p.step(ui, &[menu], &Input { pads: vec!["xbuttona".into()], ..at(220.0, 140.0) }, &no_measure).clicks;
        let key = By::Key("xbuttona".into());
        let mut p = Pointer::default();
        p.step(&mut ui, &[menu], &at(220.0, 140.0), &no_measure);
        assert_eq!(p.over, Some(cancel));
        // Cancel's own xbuttona names HiddenButton, which is hidden: the menu's (OkButton).
        assert_eq!(a(&mut p, &mut ui), [Click { tile: ok, id: Some(1.0), by: key.clone(), stamp: 0 }]);
        // Shown, it takes the key: the highlighted tile's ref comes first.
        assert!(ui.set_float("TestMenu/Main/HiddenButton/visible", 1.0));
        assert_eq!(a(&mut p, &mut ui), [Click { tile: hidden, id: Some(3.0), by: key.clone(), stamp: 0 }]);
        // Disabled (`target` 0): the menu's again.
        assert!(ui.set_float("TestMenu/Main/HiddenButton/target", 0.0));
        assert_eq!(a(&mut p, &mut ui), [Click { tile: ok, id: Some(1.0), by: key, stamp: 0 }]);
    }

    #[test]
    fn a_key_moved_highlight_goes_with_its_tile() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        let cancel = id(&ui, "TestMenu/Main/CancelButton");
        let still = at(120.0, 115.0);
        let down = Input { pads: vec!["xdown".into()], ..still.clone() };
        let press = Input { press: true, ..still.clone() };
        let mut p = Pointer::default();
        p.step(&mut ui, &[menu], &still, &no_measure);
        p.step(&mut ui, &[menu], &down, &no_measure);
        assert_eq!(p.over, Some(cancel));
        // Cancel hidden: the pointer's tile (Ok) takes the highlight back, and a press
        // clicks Ok, not the hidden Cancel.
        ui.set_trait(cancel, "visible", Value::Num(0.0));
        let r = p.step(&mut ui, &[menu], &press, &no_measure);
        assert_eq!(r.over, Some((Some(cancel), Some(ok))));
        assert_eq!(ui.get_float("TestMenu/Main/CancelButton/mouseover"), Some(0.0));
        assert_eq!(r.clicks, [Click { tile: ok, id: Some(1.0), by: By::Pointer, stamp: 0 }]);
        // Shown again and moved to by the key, then no longer a target: the same.
        ui.set_trait(cancel, "visible", Value::Num(1.0));
        p.step(&mut ui, &[menu], &down, &no_measure);
        assert_eq!(p.over, Some(cancel));
        assert!(ui.set_float("TestMenu/Main/CancelButton/target", 0.0));
        let r = p.step(&mut ui, &[menu], &press, &no_measure);
        assert_eq!(p.over, Some(ok));
        assert_eq!(r.clicks, [Click { tile: ok, id: Some(1.0), by: By::Pointer, stamp: 0 }]);
    }

    #[test]
    fn script_clicks_are_reported_at_the_next_step() {
        let (mut ui, menu) = tree();
        let ok = id(&ui, "TestMenu/Main/OkButton");
        assert_eq!(find(&ui, "TestMenu#1"), Some(ok));
        assert_eq!(find(&ui, "testmenu#2"), Some(id(&ui, "TestMenu/Main/CancelButton")));
        assert_eq!(find(&ui, "TestMenu#7"), None);
        assert_eq!(find(&ui, "TestMenu/Main/OkButton"), Some(ok));
        let mut p = Pointer::default();
        p.script_click(&mut ui, ok);
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/clicked"), Some(1.0));
        let r = p.step(&mut ui, &[menu], &Input::default(), &no_measure);
        assert_eq!(r.clicks, [Click { tile: ok, id: Some(1.0), by: By::Script, stamp: 0 }]);
        assert_eq!(ui.get_float("TestMenu/Main/OkButton/clicked"), Some(0.0));
    }

    #[test]
    fn an_unloaded_tile_is_forgotten() {
        let (mut ui, menu) = tree();
        let mut p = Pointer::default();
        p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert!(p.active(&ui).is_some());
        ui.unload_tile(menu);
        assert_eq!(p.active(&ui), None);
        let r = p.step(&mut ui, &[menu], &at(120.0, 115.0), &no_measure);
        assert_eq!(p.over, None);
        assert_eq!(r.over, None, "the tile went with its menu; nothing to write");
        // An id from a tree since replaced is not live in the new one.
        assert!(!Ui::new().is_live(menu));
    }

    #[test]
    fn text_tiles_are_measured_and_justified() {
        let src = MemSource::default().with(
            "menus\\t.xml",
            r#"<menu name="T"> <text name="Word"> <target> 1 </target> <x> 100 </x> <y> 0 </y> <justify> &center; </justify> <string> abc </string> </text> </menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "menus\\t.xml").unwrap();
        ui.update();
        let measure = |_: f64, s: &str, _: Option<f64>| (10.0 * s.len() as f64, 20.0);
        // 30 wide, centred on x 100: 85 to 115.
        assert!(hit(&ui, &[menu], (86.0, 5.0), &measure).is_some());
        assert!(hit(&ui, &[menu], (116.0, 5.0), &measure).is_none());
        assert!(hit(&ui, &[menu], (86.0, 5.0), &no_measure).is_none());
        // Entity strings written via SetUIString/SetUIStringAlt behave like Value::Sym.
        let word = ui.find_tile("T/Word").unwrap();
        ui.set_trait(word, "justify", Value::Str("&right".into()));
        // 30 wide, right-justified on x 100: 70 to 100.
        assert!(hit(&ui, &[menu], (71.0, 5.0), &measure).is_some());
        assert!(hit(&ui, &[menu], (101.0, 5.0), &measure).is_none());
        ui.set_trait(word, "visible", Value::Str("&false".into()));
        assert!(hit(&ui, &[menu], (71.0, 5.0), &measure).is_none());
        ui.set_trait(word, "visible", Value::Str("&true".into()));
        assert!(hit(&ui, &[menu], (71.0, 5.0), &measure).is_some());
    }

    #[test]
    fn a_click_keeps_the_stamp_of_when_it_was_made() {
        let src = MemSource::default().with("m.xml", r#"<menu name="M"><hotrect name="B"><target>1</target><width>9</width><height>9</height></hotrect></menu>"#);
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "m.xml").unwrap();
        ui.update();
        let b = ui.find_tile("M/B").unwrap();
        let mut p = Pointer { stamp: 5, ..Pointer::default() };
        p.script_click(&mut ui, b);
        // The screen moved on before the step reports the queued click.
        p.stamp = 6;
        let r = p.step(&mut ui, &[menu], &Input { at: Some((1.0, 1.0)), press: true, ..Input::default() }, &no_measure);
        assert_eq!(r.clicks.iter().map(|c| c.stamp).collect::<Vec<_>>(), [5, 6], "queued, then the frame's own");
    }

    #[test]
    fn a_click_lands_on_the_candidates_as_they_were_taken() {
        let src = MemSource::default().with(
            "m.xml",
            r#"<menu name="M"><hotrect name="B"><target>1</target><width>9</width><height>9</height></hotrect>
               <hotrect name="C"><target>1</target><x>20</x><width>9</width><height>9</height></hotrect></menu>"#,
        );
        let mut ui = Ui::new();
        let menu = ui.load_menu(&src, "m.xml").unwrap();
        ui.update();
        let (b, c) = (ui.find_tile("M/B").unwrap(), ui.find_tile("M/C").unwrap());
        let taken = hit_candidates(&ui, menu, &no_measure);
        assert_eq!(taken.iter().map(|t| t.under).collect::<Vec<_>>(), [b, c]);
        // The same answer as the live tree's, while it stands.
        let on = |c: &[HitCandidate], at| top_hit(&ui, c, at);
        assert_eq!(on(&taken, (1.0, 1.0)), hit(&ui, &[menu], (1.0, 1.0), &no_measure));
        assert_eq!(on(&taken, (15.0, 1.0)), None);
        // The tree moves on: the click is on what was taken, not what it became.
        ui.set_trait(b, "x", Value::Num(50.0));
        ui.update();
        assert_eq!(hit(&ui, &[menu], (1.0, 1.0), &no_measure), None);
        let mut p = Pointer::default();
        let r = p.step(&mut ui, &[menu], &Input { at: Some((1.0, 1.0)), press: true, resolved: Some(taken.clone()), ..Input::default() }, &no_measure);
        assert_eq!(r.clicks.first().map(|k| k.tile), Some(b));
        // A tile gone since is not hit.
        ui.unload_tile(b);
        let r = p.step(&mut ui, &[menu], &Input { at: Some((2.0, 2.0)), press: true, resolved: Some(taken), ..Input::default() }, &no_measure);
        assert!(r.clicks.is_empty());
    }
}
