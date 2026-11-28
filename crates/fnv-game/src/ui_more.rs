//! The rest of the tile functions (task T9; docs/design/ui-tile-tree.md §5.2,
//! docs/spec-notes/menu-xml.md §9): JIP's SetUIFloatGradual, AttachUIXML,
//! AttachUIComponent, GetStringUIDimensions; SUP's GetUIFloatGradual… readers,
//! SetUIFloatMultiple, GetUIValueType, GetUIElementType and DumpTileInfo family; CacheUI's
//! AddTraitToTile and SetUIStringReveal; xNVSE's SortUIListBox. CacheUI's cache functions
//! are in [`crate::ui_cache`], SUP's HUD bars in [`crate::ui_hudbar`].
//!
//! [`call`] takes a function by name as `crate::ui_functions::call` does; `crate::nvse::call`
//! asks it right after that one. Behaviour from the GECK wiki export's pages, cited per
//! function; what a page leaves open is tagged.

use crate::game::Game;
use crate::ui_gradients::{mode_built, Gradient, Kind, Owner};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use fnv_ui::{MemSource, Source, TileId, Ui, Value};

/// A text's measure: `(font, text, wrap width)` to `(width, height, lines)` in interface
/// units, the engine's font metrics (as `crate::ui_hud::write_text_widths` takes them).
pub type Measure = Box<dyn Fn(f64, &str, f64) -> (f64, f64, f64) + Send + Sync>;

/// The measure borrowed, as `string_dimensions` takes it.
pub type MeasureRef<'a> = dyn Fn(f64, &str, f64) -> (f64, f64, f64) + 'a;

/// A component AttachUIXML or AttachUIComponent made: injected into `HUDMainMenu`'s root
/// [documented: AttachUIXML, "Internally, the components are injected into HUDMainMenu
/// menu root"], to follow the named NiNode of the reference's model. The engine finds the
/// node on screen each frame and [`place_attachments`] puts the rect there (menu-xml.md §9).
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub reference: FormId,
    /// The NiNode's name, case-sensitive [documented: AttachUIXML notes].
    pub node: String,
    pub tile: TileId,
    /// What [`place_attachments`] last decided: `Some(true)` on screen, `Some(false)`
    /// hidden, `None` not placed yet (hidden) [design].
    pub shown: Option<bool>,
}

/// One attachment whose state [`place_attachments`] changed: shown at `at` (interface
/// units from the view's top left), or hidden (`None`). For the engine's log.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub reference: FormId,
    pub node: String,
    /// The attached rect's name.
    pub rect: String,
    pub at: Option<(f64, f64)>,
}

/// The tile functions' session state beyond the tree (not saved).
#[derive(Default)]
pub struct UiExtra {
    pub cache: crate::ui_cache::Cache,
    pub bars: crate::ui_hudbar::Bars,
    pub attached: Vec<Attachment>,
    /// Bounded model-projection requests; transient and cleared with this UI tree.
    pub projections: crate::ui_projection::Requests,
    /// The engine's text measure for GetStringUIDimensions; `None` headless (the function
    /// then writes zero dimensions [design: deterministic unavailable-font policy]).
    pub measure: Option<Measure>,
}

pub(crate) fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        Some(XVal::Num(n)) => Value::Num(*n).text(),
        _ => String::new(),
    }
}

/// A number argument; `None` when it was not given.
pub(crate) fn opt(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k)? {
        XVal::Num(n) => Some(*n),
        XVal::Str(s) => Some(s.trim().parse().unwrap_or(0.0)),
        XVal::Ref(r) => Some(f64::from(*r)),
        XVal::Array(_) => Some(0.0),
    }
}

pub(crate) fn num(args: &[XVal], k: usize) -> f64 {
    opt(args, k).unwrap_or(0.0)
}

/// A number argument that must be a real number: a NaN or an infinity (a script's bad
/// arithmetic) reads as 0, so it never gets into a trait or a change's time [design].
pub(crate) fn finite(args: &[XVal], k: usize) -> f64 {
    finite_value(num(args, k))
}

/// A non-finite number (an input, or a sum) as 0 [design: as `finite`].
pub(crate) fn finite_value(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

pub(crate) fn yes(b: bool) -> XVal {
    XVal::Num(if b { 1.0 } else { 0.0 })
}

/// A trait's name as the tile holds it.
pub(crate) fn trait_name(ui: &Ui, tile: TileId, k: usize) -> String {
    ui.tile(tile).traits[k].name.clone()
}

/// The gradual functions' tail `[start [end [duration [mode]]]]` on a trait: none stops
/// the change, a start alone stops it and sets the start, more starts it [documented:
/// SetUIFloatGradual, CacheUIGradual: "call the function again while omitting all but"
/// the path (the index and trait), "Alternatively, do include the 2nd argument"]. A
/// missing duration is 0, the end at once (see docs/spec-notes/menu-xml.md §9). A start refused by a lock or for a
/// mode not built gives false.
pub(crate) fn gradual(g: &mut Game, owner: Owner, tile: TileId, name: &str, rest: &[XVal]) -> bool {
    match rest.len() {
        0 => g.ui_gradients.stop(owner, tile, name),
        1 => {
            g.ui_gradients.stop(owner, tile, name);
            g.ui_tree.set_trait(tile, name, Value::Num(finite(rest, 0)));
        }
        _ => {
            let mode = num(rest, 3).max(0.0) as u8;
            // JIP's page lists modes 0-3; another reads as 0 there [hypothesis].
            let mode = if owner == Owner::Jip && mode > 3 { 0 } else { mode };
            if !mode_built(mode) || g.ui_gradients.locked(owner, tile, name) {
                return false;
            }
            let kind = Kind::Float { start: Some(finite(rest, 0)), end: finite(rest, 1), mode };
            let gr = Gradient { owner, tile, name: name.to_string(), kind, duration: finite(rest, 2), elapsed: 0.0, delay: 0.0, lock: false, watch: None, yields: false };
            g.ui_gradients.start(&mut g.ui_tree, gr);
        }
    }
    true
}

/// CacheUIGradualDelay's tail `delay [end [duration [flag [mode [match]]]]]`: after the
/// delay the trait goes from its value then to `end` [documented: CacheUIGradualDelay].
/// Without an end it stops the trait's changes, locked ones too; flag 0 gives way to a
/// running change, 1 replaces it, 2 replaces it and locks until it finishes; `match` names
/// a trait of the tile whose change during the delay ends it [documented: the page and
/// its notes].
pub(crate) fn gradual_delay(g: &mut Game, tile: TileId, name: &str, rest: &[XVal]) -> bool {
    let owner = Owner::CacheUi;
    if rest.len() < 2 {
        g.ui_gradients.stop(owner, tile, name);
        return true;
    }
    let flag = num(rest, 3) as i64;
    let mode = num(rest, 4).max(0.0) as u8;
    if !mode_built(mode) || (flag != 2 && g.ui_gradients.locked(owner, tile, name)) {
        return false;
    }
    if flag == 0 && g.ui_gradients.active(owner, tile, name).is_some() {
        return false;
    }
    let watch = (rest.len() > 5).then(|| {
        let m = text(rest, 5);
        let v = g.ui_tree.tile(tile).get(&m).cloned().unwrap_or_default();
        (m, v)
    });
    let kind = Kind::Float { start: None, end: finite(rest, 1), mode };
    let gr = Gradient { owner, tile, name: name.to_string(), kind, duration: finite(rest, 2), elapsed: 0.0, delay: finite(rest, 0).max(0.0), lock: flag == 2, watch, yields: flag == 0 };
    g.ui_gradients.start(&mut g.ui_tree, gr);
    true
}

/// The reveal functions' tail `[text [time [mode [invert]]]]`: none stops, the text alone
/// stops and sets it, more starts it [documented: SetUIStringReveal,
/// CacheUIStringReveal: "To stop a reveal use either args"].
pub(crate) fn reveal(g: &mut Game, tile: TileId, name: &str, rest: &[XVal]) -> bool {
    let owner = Owner::CacheUi;
    match rest.len() {
        0 => g.ui_gradients.stop(owner, tile, name),
        1 => {
            g.ui_gradients.stop(owner, tile, name);
            g.ui_tree.set_trait(tile, name, Value::Str(text(rest, 0)));
        }
        _ => {
            let kind = Kind::Reveal { text: text(rest, 0), mode: (num(rest, 2).max(0.0) as u8).min(3), invert: num(rest, 3) != 0.0 };
            let gr = Gradient { owner, tile, name: name.to_string(), kind, duration: finite(rest, 1), elapsed: 0.0, delay: 0.0, lock: false, watch: None, yields: false };
            g.ui_gradients.start(&mut g.ui_tree, gr);
        }
    }
    true
}

/// SUP's element types: 1 rect, 2 image, 3 text, 4 3D, 5 menu [documented:
/// GetUIElementType, SetUIFloatMultiple]; `nif` is the 3D tile; `hotrect`, `radial`,
/// `window` are in no list and read 0 (see docs/spec-notes/menu-xml.md §9).
pub fn element_type(kind: &str) -> i64 {
    match kind {
        "rect" => 1,
        "image" => 2,
        "text" => 3,
        "nif" => 4,
        "menu" => 5,
        _ => 0,
    }
}

/// GetUIValueType: 1 a number, 2 a string, 3 an action (a trait worked out by operators,
/// "not given a static value") [documented: GetUIValueType]. A constant reads as a number
/// [hypothesis: menu-xml.md §2.3, the original holds them as numbers].
pub fn value_type(ui: &Ui, tile: TileId, k: usize) -> i64 {
    let t = &ui.tile(tile).traits[k];
    if t.expr.is_some() {
        3
    } else if matches!(t.value, Value::Str(_)) {
        2
    } else {
        1
    }
}

/// A trait's value as DumpTileInfo prints it: numbers with six decimals [documented:
/// DumpTileInfoToArray's example output, "[ visible ] : 1.000000"], strings as they are.
fn dump_value(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        other => format!("{:.6}", other.num()),
    }
}

/// DumpTileInfo's lines for one tile: its name, then every trait [documented:
/// DumpTileInfoToArray's example output, `[ Tilename ] : MCM_Trigger` then `[ trait ] :
/// value`; hypothesis: the console dump prints the same lines].
pub fn dump_lines(ui: &Ui, tile: TileId) -> Vec<String> {
    let t = ui.tile(tile);
    let mut lines = vec![format!("[ Tilename ] : {}", t.name)];
    lines.extend(t.traits.iter().map(|tr| format!("[ {} ] : {}", tr.name, dump_value(&tr.value))));
    lines
}

/// DumpTileInfoALL's lines: every live menu's tiles, each preceded by its path [design:
/// the page says only "all XML elements which are currently loaded"].
pub fn dump_all_lines(ui: &Ui) -> Vec<String> {
    let mut lines = Vec::new();
    let menus: Vec<TileId> = ui.menus().collect();
    for m in menus {
        let mut stack = vec![m];
        while let Some(t) = stack.pop() {
            lines.push(format!("{}:", ui.path_of(t)));
            lines.extend(dump_lines(ui, t));
            stack.extend(ui.tile(t).children.iter().rev().copied());
        }
    }
    lines
}

/// Where a dump goes: the console, or with DumpToFile the log (SUP writes `SUPNVSE.log` in
/// the game's folder [documented: DumpTileInfo]; NVForever never writes there and logs
/// to its own log instead [design: CLAUDE.md]).
pub(crate) fn emit(g: &mut Game, to_file: bool, lines: Vec<String>) {
    if to_file {
        for l in &lines {
            tracing::info!(target: "nvforever::ui", "{l}");
        }
    } else {
        g.console_out.extend(lines);
    }
}

/// The key SortUIListBox sorts an item by.
#[derive(Clone, Debug, PartialEq)]
enum SortKey {
    Num(f64),
    Text(String),
}

struct KeySpec {
    path: String,
    reverse: bool,
    numeric: bool,
    item_name: bool,
}

/// An item name without its quantity and mod marks: "9mm Pistol (2)" and "9mm Pistol+"
/// both "9mm Pistol" [documented: SortUIListBox, `@`].
pub fn bare_item_name(name: &str) -> String {
    let mut s = name.trim();
    loop {
        let before = s;
        if let Some(open) = s.rfind(" (") {
            let inner = &s[open + 2..];
            if inner.ends_with(')') && inner[..inner.len() - 1].chars().all(|c| c.is_ascii_digit()) && inner.len() > 1 {
                s = s[..open].trim_end();
            }
        }
        s = s.trim_end_matches('+').trim_end();
        if s == before {
            return s.to_string();
        }
    }
}

/// SortUIListBox: `"key[,key…][$destination[$increment]]"`, each key `[-][#][@]path`;
/// `-` reverses it, `#` sorts numerically (else alphabetically), `@` by the item name
/// without quantity and mod marks; the destination trait (default `_y`) of each item is
/// set in the sorted order, each the last one's plus the last one's increment trait
/// (default `height`) [documented: SortUIListBox notes] (see docs/spec-notes/menu-xml.md §9). The items are the
/// list's children that have the destination trait (a list's other children, its
/// scroll bar, are not placed by it); the first sorted item takes the smallest
/// destination value the items had, so the list stays where it was; alphabetic keys
/// ignore case; a key an item lacks reads 0 or empty; equal items keep their order; the
/// children are not reordered (the page names only a trait to change). False when the
/// path names no tile. Numeric keys compare by `total_cmp`: a total order, NaN keys at
/// the ends [design: `sort_by` needs one].
pub fn sort_list_box(ui: &mut Ui, path: &str, spec: &str) -> bool {
    let Some(list) = ui.find_tile(path) else { return false };
    let mut parts = spec.split('$');
    let keys: Vec<KeySpec> = parts
        .next()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| {
            let mut spec = KeySpec { path: String::new(), reverse: false, numeric: false, item_name: false };
            let mut rest = k;
            while let Some(c) = rest.chars().next().filter(|c| matches!(c, '-' | '#' | '@')) {
                match c {
                    '-' => spec.reverse = true,
                    '#' => spec.numeric = true,
                    _ => spec.item_name = true,
                }
                rest = &rest[1..];
            }
            spec.path = rest.trim().to_string();
            spec
        })
        .collect();
    let dest = parts.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("_y").to_string();
    let inc = parts.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("height").to_string();
    let items: Vec<TileId> = ui.tile(list).children.iter().copied().filter(|&c| ui.tile(c).live && ui.find_trait_in(c, &dest).is_some()).collect();
    if items.is_empty() {
        return true;
    }
    let read = |ui: &Ui, item: TileId, p: &str| ui.find_trait_in(item, p).map(|(t, k)| ui.tile(t).traits[k].value.clone());
    let key_of = |ui: &Ui, item: TileId| -> Vec<SortKey> {
        keys.iter()
            .map(|k| {
                let v = read(ui, item, &k.path).unwrap_or_default();
                if k.numeric {
                    SortKey::Num(v.num())
                } else if k.item_name {
                    SortKey::Text(bare_item_name(&v.text()).to_lowercase())
                } else {
                    SortKey::Text(v.text().to_lowercase())
                }
            })
            .collect()
    };
    let mut sorted: Vec<(TileId, Vec<SortKey>)> = items.iter().map(|&i| (i, key_of(ui, i))).collect();
    sorted.sort_by(|a, b| {
        for (k, spec) in keys.iter().enumerate() {
            let o = match (&a.1[k], &b.1[k]) {
                (SortKey::Num(a), SortKey::Num(b)) => a.total_cmp(b),
                (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
                _ => unreachable!("each key has one type"),
            };
            let o = if spec.reverse { o.reverse() } else { o };
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    let mut at = items.iter().filter_map(|&i| read(ui, i, &dest)).map(|v| v.num()).fold(f64::INFINITY, f64::min);
    if !at.is_finite() {
        at = 0.0;
    }
    for (item, _) in sorted {
        if let Some((t, k)) = ui.find_trait_in(item, &dest) {
            let name = trait_name(ui, t, k);
            ui.set_trait(t, &name, Value::Num(at));
        }
        at += read(ui, item, &inc).map_or(0.0, |v| v.num());
    }
    true
}

/// GetStringUIDimensions' outputs: width, height and lines of `text` in font `font`
/// wrapped at `wrap` [documented: GetStringUIDimensions], by the engine's measure.
pub fn string_dimensions(measure: &MeasureRef<'_>, text: &str, font: f64, wrap: f64) -> (f64, f64, f64) {
    measure(font, text, wrap)
}

/// Whether XML is a single `rect` with everything else in it [documented: AttachUIXML,
/// AttachUIComponent, "must be composed of a single rect parent"].
pub fn single_rect(xml: &str) -> bool {
    let doc = fnv_ui::xml::parse(xml);
    let mut elements = doc.nodes.iter().filter_map(|n| match n {
        fnv_ui::xml::Node::Element(e) => Some(e),
        _ => None,
    });
    matches!((elements.next(), elements.next()), (Some(e), None) if e.is("rect"))
}

/// Reads a menu file as InjectUIXML does: under the game's folder (`data\…`) or `Data`.
fn read_xml(src: &dyn Source, file: &str) -> Option<String> {
    let f = file.replace('/', "\\");
    let f = match f.get(..5) {
        Some(head) if head.eq_ignore_ascii_case("data\\") => &f[5..],
        _ => &f[..],
    };
    src.read(f).map(|b| fnv_ui::xml::decode(&b))
}

/// AttachUIXML / AttachUIComponent: the XML into `HUDMainMenu`'s root, kept as attached to
/// the reference's node; 1 on success [documented: AttachUIXML]. Without a calling
/// reference, the HUD, or a single rect, 0. Replaces the one attached before to the same
/// reference, node and rect name (a mod attaching per frame grows nothing), and keeps it
/// `visible` 0 until [`place_attachments`] finds its node on screen [design: the
/// maintainer's review of #1224].
fn attach(g: &mut Game, subject: Option<FormId>, node: &str, xml: &str) -> bool {
    const HUD: &str = "HUDMainMenu";
    let Some(reference) = subject else { return false };
    if !single_rect(xml) {
        return false;
    }
    let Some(hud) = g.ui_tree.menu(HUD) else { return false };
    let before: Vec<TileId> = g.ui_tree.tile(hud).children.clone();
    let empty = MemSource::default();
    let src: &dyn Source = match g.ui_files.as_deref() {
        Some(s) => s,
        None => &empty,
    };
    if !g.ui_tree.inject_text(src, HUD, xml) {
        return false;
    }
    let made = g.ui_tree.tile(hud).children.iter().copied().find(|c| !before.contains(c));
    match made {
        Some(tile) => {
            let name = g.ui_tree.tile(tile).name.clone();
            g.ui_extra.attached.retain(|a| {
                if a.reference == reference && a.node == node && g.ui_tree.tile(a.tile).name == name {
                    g.ui_tree.unload_tile(a.tile);
                    false
                } else {
                    true
                }
            });
            g.ui_tree.set_trait(tile, "visible", Value::Num(0.0));
            // The rect's place is the node's from now on ([`place_attachments`]): the XML's
            // `x` and `y` lose any expression, so the tree's working out (before each
            // drawing) cannot put it back over the placement. Each stays a plain number at
            // what it held now (a constant as written, an expression as last worked out, 0
            // when never), so scripts read and write it before the first placement and while
            // the node is not found [design].
            for name in ["x", "y"] {
                let held = g.ui_tree.tile(tile).get(name).map_or(0.0, Value::num);
                g.ui_tree.remove_trait(tile, name);
                g.ui_tree.set_trait(tile, name, Value::Num(if held.is_finite() { held } else { 0.0 }));
            }
            g.ui_extra.attached.push(Attachment { reference, node: node.to_string(), tile, shown: None });
            true
        }
        None => false,
    }
}

/// Drops the attachments whose reference is gone (deleted, disabled, or out of the loaded
/// cells, as `Game::present` and `Game::active_refs` judge it) each frame, and keeps the
/// rest that [`place_attachments`] has not put on screen `visible` 0 (an expression in the
/// rect's `visible` would show it again when its sources change) [design: the maintainer's
/// review of #1224].
pub fn prune_attachments(g: &mut Game) {
    let mut attached = std::mem::take(&mut g.ui_extra.attached);
    attached.retain(|a| {
        if !g.ui_tree.tile(a.tile).live || !g.present(a.reference) || !g.cell_of(a.reference).is_some_and(|c| g.loaded_cells.contains(&c)) {
            g.ui_tree.unload_tile(a.tile);
            false
        } else {
            if a.shown != Some(true) {
                hide(&mut g.ui_tree, a.tile);
            }
            true
        }
    });
    g.ui_extra.attached = attached;
}

/// `visible` 0, written only when something showed it: a write a frame would stir the tree.
fn hide(ui: &mut Ui, tile: TileId) {
    if ui.tile(tile).get("visible").is_none_or(|v| v.num() != 0.0) {
        ui.set_trait(tile, "visible", Value::Num(0.0));
    }
}

/// A trait set to the number `v` unless it holds it already (no write, so no stir).
fn set_num(ui: &mut Ui, tile: TileId, name: &str, v: f64) {
    if ui.tile(tile).get(name).is_none_or(|old| old.num() != v) {
        ui.set_trait(tile, name, Value::Num(v));
    }
}

/// Where the tiles above `tile` put its `x` 0 and `y` 0: their `x` and `y` summed, as the
/// engine's layout adds them (`nvf-play` `tiles/layout.rs`, `place`) [design].
fn origin_of(ui: &Ui, tile: TileId) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    let mut at = ui.tile(tile).parent;
    while let Some(id) = at {
        let t = ui.tile(id);
        x += t.get("x").map_or(0.0, Value::num);
        y += t.get("y").map_or(0.0, Value::num);
        at = t.parent;
    }
    (x, y)
}

/// Puts each attached rect over its node. `at(reference, node)` is the node's point on
/// screen this frame in interface units from the view's top left (the space `screen()`
/// and the HUD's tiles are in, 960 to the view's height), or `None` when the rect is not
/// to be shown: no such node loaded, the reference not drawn, no camera, behind the
/// camera or off the screen (the engine decides: `nvforever` `ui_attach.rs`).
///
/// [documented: AttachUIComponent] The component follows the node; the page does not say
/// which point of the rect is on the node. [design] The rect's `x` and `y` (its top left)
/// are put at the node's point (they were made plain numbers when it was attached, any
/// expression dropped, so the tree's working out never moves it), so a component centres
/// itself on the node through its children's places; they are written only when they
/// change, and a script's write to them lasts until the next placement. [design] `visible` is written 1 when the rect comes on screen and 0 when
/// it leaves (and held at 0 while it is off, [`prune_attachments`]); a script that hides
/// the rect while it stays on screen keeps it hidden until it next comes on screen.
///
/// Returns the attachments whose state changed, or that were placed for the first time,
/// for the engine's log.
pub fn place_attachments(g: &mut Game, mut at: impl FnMut(FormId, &str) -> Option<(f64, f64)>) -> Vec<Placement> {
    let mut changed = Vec::new();
    let ui = &mut g.ui_tree;
    for a in &mut g.ui_extra.attached {
        if !ui.tile(a.tile).live {
            continue;
        }
        let point = at(a.reference, &a.node);
        match point {
            Some((x, y)) => {
                let (ox, oy) = origin_of(ui, a.tile);
                set_num(ui, a.tile, "x", x - ox);
                set_num(ui, a.tile, "y", y - oy);
                if a.shown != Some(true) {
                    ui.set_trait(a.tile, "visible", Value::Num(1.0));
                }
            }
            None => hide(ui, a.tile),
        }
        let shown = Some(point.is_some());
        if a.shown != shown {
            a.shown = shown;
            changed.push(Placement { reference: a.reference, node: a.node.clone(), rect: ui.tile(a.tile).name.clone(), at: point });
        }
    }
    changed
}

/// The tree was built again (`crate::ui_loading`): the changes, the cache entries and
/// the attachments named the old tiles [documented: CacheUI, "Cache will be flushed on
/// gamereload"; hypothesis for the others]; the HUD bars are made again in the new HUD
/// ([`crate::ui_hudbar::rebuilt`]).
pub fn tree_rebuilt(g: &mut Game) {
    g.ui_gradients.clear();
    g.ui_extra.cache.clear();
    g.ui_extra.attached.clear();
    g.ui_extra.projections.clear();
    crate::ui_hudbar::rebuilt(g);
}

/// The functions here that read or change the tree, in lower case (the HUD's kept-back
/// writes are made first, as `crate::ui_functions` does).
const TREE_FUNCTIONS: [&str; 16] = [
    "setuifloatgradual",
    "getuifloatgradualactivefortile",
    "getuifloatgradualactivefortiletrait",
    "getuifloatgradualmode",
    "getuifloatgradualtimer",
    "setuistringreveal",
    "setuifloatmultiple",
    "attachuixml",
    "attachuicomponent",
    "addtraittotile",
    "getuivaluetype",
    "getuielementtype",
    "sortuilistbox",
    "dumptileinfo",
    "dumptileinfoall",
    "dumptileinfotoarray",
];

/// Resolve inspection commands' path before the HUD's deferred writes are flushed.
/// [documented: GetUIElementType, DumpTileInfo] Missing and nonexistent paths use
/// the cursor; Pointer::active rejects unloaded tiles [design].
fn inspection_tile(g: &Game, path: &str) -> Option<TileId> {
    g.ui_tree.find_tile(path).or_else(|| g.ui_pointer.active(&g.ui_tree))
}

/// The function `name` (any case) with its arguments; `None` when it is not one of these.
pub fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let lname = name.to_ascii_lowercase();
    let path_arg = if lname == "dumptileinfo" { 1 } else { 0 };
    // The HUD's kept-back writes are made first when the function touches the HUD (as
    // `crate::ui_functions` does): the HUD bars and the attachments always, a cached tile
    // (any menu's, the HUD's as a rule) [design], the path functions by their path; a
    // function on another menu leaves the HUD's writes kept back.
    let cursor_query = matches!(lname.as_str(), "getuielementtype" | "dumptileinfo");
    let inspected = cursor_query.then(|| inspection_tile(g, &text(args, path_arg))).flatten();
    let on_hud = inspected.is_some_and(|t| crate::ui_functions::on_hud(&g.ui_tree.path_of(t)))
        || lname.starts_with("hudbar")
        || lname.starts_with("cacheui")
        || matches!(lname.as_str(), "attachuixml" | "attachuicomponent" | "dumptileinfoall")
        || (!cursor_query && TREE_FUNCTIONS.contains(&lname.as_str()) && crate::ui_functions::on_hud(&text(args, path_arg)));
    if on_hud {
        g.ui_used = true;
        crate::ui_hud::flush(g);
    }
    if let Some(r) = crate::ui_cache::call(g, &lname, args) {
        return Some(r);
    }
    if let Some(r) = crate::ui_hudbar::call(g, &lname, args) {
        return Some(r);
    }
    let path = text(args, 0);
    let zero = XVal::Num(0.0);
    let gradient = |g: &Game, owner: Owner| {
        let (t, k) = g.ui_tree.find_trait(&path)?;
        let name = trait_name(&g.ui_tree, t, k);
        g.ui_gradients.active(owner, t, &name).cloned()
    };
    Some(Ok(match lname.as_str() {
        // [documented: SetUIFloatGradual] `path [start [end [duration [mode]]]]`. No return
        // value is documented: 1 when the trait is there [design].
        "setuifloatgradual" => match g.ui_tree.find_trait(&path) {
            Some((t, k)) => {
                let name = trait_name(&g.ui_tree, t, k);
                yes(gradual(g, Owner::Jip, t, &name, args.get(1..).unwrap_or(&[])))
            }
            None => zero,
        },
        // [documented: GetUIFloatGradualActiveForTile, …ForTileTrait]
        "getuifloatgradualactivefortile" => yes(g.ui_tree.find_tile(&path).is_some_and(|t| g.ui_gradients.on_tile(Owner::Jip, t))),
        "getuifloatgradualactivefortiletrait" => yes(gradient(g, Owner::Jip).is_some()),
        // -1 when the trait is not changing [documented: GetUIFloatGradualMode].
        "getuifloatgradualmode" => XVal::Num(gradient(g, Owner::Jip).map_or(-1.0, |gr| f64::from(gr.mode()))),
        // The time since the change began, or with 1 the time left [documented:
        // GetUIFloatGradualTimer]; a repeating change's time left is the current round's
        // and -1 when nothing changes [hypothesis].
        "getuifloatgradualtimer" => XVal::Num(match gradient(g, Owner::Jip) {
            Some(gr) if num(args, 1) != 0.0 => {
                let d = gr.duration.max(0.0);
                if gr.repeats() && d > 0.0 {
                    d - gr.elapsed % d
                } else {
                    (d - gr.elapsed).max(0.0)
                }
            }
            Some(gr) => gr.elapsed,
            None => -1.0,
        }),
        // [documented: SetUIStringReveal] `path [text [time [mode [invert]]]]`.
        "setuistringreveal" => match g.ui_tree.find_trait(&path) {
            Some((t, k)) => {
                let name = trait_name(&g.ui_tree, t, k);
                yes(reveal(g, t, &name, args.get(1..).unwrap_or(&[])))
            }
            None => zero,
        },
        // `path trait value [type]`: every tile in the one named, of the type when given
        // [documented: SetUIFloatMultiple] (see docs/spec-notes/menu-xml.md §9). Only tiles that have the trait
        // (as SetUIFloat makes none), not the named tile itself; the count changed is
        // returned [design: no return value is documented].
        // A non-finite value writes 0 [design: `finite`].
        "setuifloatmultiple" => match g.ui_tree.find_tile(&path) {
            Some(root) => {
                let trait_ = text(args, 1);
                let v = finite(args, 2);
                let filter = opt(args, 3).map(|t| t as i64).filter(|&t| t != 0);
                let mut stack: Vec<TileId> = g.ui_tree.tile(root).children.clone();
                let mut n = 0;
                while let Some(t) = stack.pop() {
                    stack.extend(g.ui_tree.tile(t).children.iter().copied());
                    let tile = g.ui_tree.tile(t);
                    if filter.is_some_and(|f| f != element_type(&tile.kind)) {
                        continue;
                    }
                    if let Some(k) = tile.trait_index(&trait_) {
                        let name = trait_name(&g.ui_tree, t, k);
                        g.ui_tree.set_trait(t, &name, Value::Num(v));
                        n += 1;
                    }
                }
                XVal::Num(f64::from(n))
            }
            None => zero,
        },
        // `ref.AttachUIXML node file` / `ref.AttachUIComponent node "xml"` (the VM formats
        // the component) [documented: AttachUIXML, AttachUIComponent].
        "attachuixml" => {
            let xml = g.ui_files.as_deref().and_then(|src| read_xml(src, &text(args, 1)));
            yes(xml.is_some_and(|x| attach(g, subject, &path, &x)))
        }
        "attachuicomponent" => {
            let xml = text(args, 1);
            yes(attach(g, subject, &path, &xml))
        }
        // `path trait value`: the trait made on the tile, or set when it is there
        // [documented: AddTraitToTile]. The page allows only traits "which already has a
        // ID assign"; the original's trait ids are not known, so any name is taken
        // (see docs/spec-notes/menu-xml.md §9). 1 when the tile is there [design: no return value is documented].
        // An empty name makes no trait (0); a non-finite value writes 0 [design].
        "addtraittotile" => match g.ui_tree.find_tile(&path) {
            Some(t) if !text(args, 1).trim().is_empty() => {
                g.ui_tree.set_trait(t, &text(args, 1), Value::Num(finite(args, 2)));
                yes(true)
            }
            _ => zero,
        },
        // -1 when no value is there [documented: GetUIValueType notes].
        "getuivaluetype" => XVal::Num(g.ui_tree.find_trait(&path).map_or(-1.0, |(t, k)| value_type(&g.ui_tree, t, k) as f64)),
        // Missing or nonexistent path falls back to the live highlighted tile
        // [documented: GetUIElementType]. No tile returns -1 [design].
        "getuielementtype" => XVal::Num(inspected.map_or(-1.0, |t| element_type(&g.ui_tree.tile(t).kind) as f64)),
        "sortuilistbox" => yes(sort_list_box(&mut g.ui_tree, &path, &text(args, 1))),
        // `text font wrap` and three output variables, which the VM takes off
        // (`fnv_script::vm`'s out_call): the answer is [0, width, height, lines].
        "getstringuidimensions" => {
            // Always supply the output slots, including headless/unavailable metrics
            // [design: never leave a caller's previous dimensions in place].
            let (w, h, n) = g.ui_extra.measure.as_ref().map_or((0.0, 0.0, 0.0), |m| {
                string_dimensions(m.as_ref(), &text(args, 0), num(args, 1), num(args, 2))
            });
            let entries = [0.0, w, h, n].iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(*v))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        },
        // `[toFile [path]]`; missing or nonexistent path uses the cursor's live tile
        // [documented: DumpTileInfo].
        "dumptileinfo" => {
            let to_file = num(args, 0) != 0.0;
            if let Some(t) = inspected {
                let lines = dump_lines(&g.ui_tree, t);
                emit(g, to_file, lines);
            }
            zero
        }
        "dumptileinfoall" => {
            let lines = dump_all_lines(&g.ui_tree);
            emit(g, num(args, 0) != 0.0, lines);
            zero
        }
        // A string map of `Tilename` and every trait [documented: DumpTileInfoToArray];
        // numbers as numbers, strings as strings [hypothesis]; empty when the path names
        // no tile.
        "dumptileinfotoarray" => {
            let mut entries = Vec::new();
            if let Some(t) = g.ui_tree.find_tile(&path) {
                let tile = g.ui_tree.tile(t);
                entries.push((XKey::Str("Tilename".into()), XVal::Str(tile.name.clone())));
                for tr in &tile.traits {
                    let v = match &tr.value {
                        Value::Str(s) => XVal::Str(s.clone()),
                        other => XVal::Num(other.num()),
                    };
                    entries.push((XKey::Str(tr.name.clone()), v));
                }
            }
            XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
        }
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_names_lose_quantity_and_mod_marks() {
        assert_eq!(bare_item_name("9mm Pistol (2)"), "9mm Pistol");
        assert_eq!(bare_item_name("9mm Pistol+"), "9mm Pistol");
        assert_eq!(bare_item_name("Pipe (Long)"), "Pipe (Long)");
    }

    #[test]
    fn a_single_rect() {
        assert!(single_rect("<rect name=\"A\"> <text name=\"B\"/> </rect>"));
        assert!(!single_rect("<rect name=\"A\"/> <rect name=\"B\"/>"));
        assert!(!single_rect("<image name=\"A\"/>"));
    }

    #[test]
    fn numeric_sort_with_nan_keys_has_a_total_order() {
        let mut xml = String::from("<menu name=\"List\">");
        for k in 0..128 {
            let n = 128 - (k * 37) % 128;
            let key = if n % 4 == 0 { "nan".to_string() } else { n.to_string() };
            xml.push_str(&format!("<rect name=\"Item{k}\"><_y>{k}</_y><height>1</height><string>{key}</string></rect>"));
        }
        xml.push_str("</menu>");
        let src = MemSource::default().with("list.xml", &xml);
        let mut ui = Ui::new();
        let list = ui.load_menu(&src, "list.xml").unwrap();
        ui.update();
        let children = ui.tile(list).children.clone();
        for &t in &children {
            if ui.tile(t).get("string").unwrap().num().is_nan() {
                ui.set_trait(t, "string", Value::Str("nan".into()));
            }
        }
        for spec in ["#string", "-#string"] {
            assert!(sort_list_box(&mut ui, "List", spec));
            let mut placed = children.clone();
            placed.sort_by_key(|&t| ui.tile(t).get("_y").unwrap().num() as i64);
            let mut expected: Vec<String> = (1..128).filter(|n| n % 4 != 0).map(|n| n.to_string()).collect();
            expected.extend(vec!["nan".to_string(); 32]);
            if spec.starts_with('-') {
                expected.reverse();
            }
            let keys: Vec<String> = placed.iter().map(|&t| ui.tile(t).get("string").unwrap().text()).collect();
            assert_eq!(keys, expected);
        }
    }
}
