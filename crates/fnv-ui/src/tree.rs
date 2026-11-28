//! The tile tree: menus read from their files into tiles with traits, found by path,
//! read and changed by the engine and by scripts, and worked out again when what a
//! trait reads from changes (docs/design/ui-tile-tree.md §2–§4,
//! docs/spec-notes/menu-xml.md).

use crate::expr::{self, Arg, Op, OpKind, Src};
use crate::value::Value;
use crate::xml::{self, Element, Node};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::num::NonZeroU64;
use std::panic::Location;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

/// The tile kinds, the tags that make a tile [observed: the tags with a `name`
/// attribute in the vanilla files; `window` is in no vanilla file but in older Gamebryo
/// menu files, kept as a tile in case a mod uses it (hypothesis)].
pub const TILE_KINDS: &[&str] = &["menu", "rect", "image", "text", "nif", "radial", "hotrect", "window"];

/// Includes nested deeper than this are not followed [design: a guard against a file
/// including itself].
const MAX_INCLUDE_DEPTH: usize = 16;

/// Passes of [`Ui::update`] before it gives up on traits that keep changing (a loop of
/// traits reading each other) [design].
const MAX_PASSES: usize = 32;

/// Where the menu files come from: the game's archives and loose files under `Data`
/// (the engine passes its VFS), or files in memory for tests.
pub trait Source {
    /// [design] Files recursively below a Data-relative directory, as full Data-relative
    /// paths; comparisons ignore case and slash style. Sources may omit discovery.
    fn list(&self, _dir: &str) -> Vec<String> {
        Vec::new()
    }
    /// A file by its path under `Data`, e.g. `menus\prefabs\meter.xml`; any case, `\`
    /// or `/`.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// [design] Whether the winning file is loose; sources without provenance return false.
    fn is_loose(&self, _path: &str) -> bool {
        false
    }
    /// [design] The archived copy of a file, also when a loose file replaces it (the
    /// file a loose replacement is compared against, [`Ui::archived_name_paths`]); `None`
    /// when no archive has it. By default a path that is not loose reads as it wins.
    fn read_archived(&self, path: &str) -> Option<Vec<u8>> {
        if self.is_loose(path) {
            None
        } else {
            self.read(path)
        }
    }
    /// A game setting's string, for `&-sName;` (the GMST `sName`): the engine answers
    /// from a [`crate::GameStrings`] (the load order's GMSTs, then the built-in table).
    fn game_string(&self, _name: &str) -> Option<String> {
        None
    }
    /// [design] Whether [`Source::game_string`] answers `name` from the built-in table, no
    /// plugin defining it. A source that cannot tell says no: its text then counts as chosen.
    fn game_string_is_built_in(&self, _name: &str) -> bool {
        false
    }
}

/// Files in memory, by path in lower case with `\`.
#[derive(Default)]
pub struct MemSource {
    pub files: HashMap<String, Vec<u8>>,
    pub strings: HashMap<String, String>,
    /// [design] The names in `strings` answering as the built-in table does, in lower case.
    pub built_in_strings: HashSet<String>,
    /// [design] Loose-file provenance for synthetic sources, using normalized paths.
    pub loose_files: HashSet<String>,
    /// [design] The archived copies under synthetic loose files, by normalized path.
    pub archived: HashMap<String, Vec<u8>>,
}

impl MemSource {
    pub fn with(mut self, path: &str, text: &str) -> MemSource {
        self.files.insert(norm(path), text.as_bytes().to_vec());
        self
    }

    /// [design] Add a synthetic loose file.
    pub fn with_loose(mut self, path: &str, text: &str) -> MemSource {
        self.loose_files.insert(norm(path));
        self.files.insert(norm(path), text.as_bytes().to_vec());
        self
    }

    /// [design] Add a synthetic loose file over an archived copy of the same path.
    pub fn with_loose_over(mut self, path: &str, loose: &str, archived: &str) -> MemSource {
        self.archived.insert(norm(path), archived.as_bytes().to_vec());
        self.with_loose(path, loose)
    }
}

impl Source for MemSource {
    fn list(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}\\", norm(dir).trim_end_matches('\\'));
        let mut paths: Vec<_> = self.files.keys().filter(|p| p.starts_with(&prefix)).cloned().collect();
        paths.sort();
        paths
    }
    fn is_loose(&self, path: &str) -> bool {
        self.loose_files.contains(&norm(path))
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(&norm(path)).cloned()
    }

    fn read_archived(&self, path: &str) -> Option<Vec<u8>> {
        let path = norm(path);
        if self.loose_files.contains(&path) {
            self.archived.get(&path).cloned()
        } else {
            self.files.get(&path).cloned()
        }
    }

    /// Without case, as the original [observed: menu-xml.md §2.4].
    fn game_string(&self, name: &str) -> Option<String> {
        self.strings.get(name).or_else(|| self.strings.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)).cloned()
    }

    fn game_string_is_built_in(&self, name: &str) -> bool {
        self.built_in_strings.contains(&name.to_ascii_lowercase())
    }
}

/// [design] A source's archived copies alone ([`Source::read_archived`]): the menu files
/// as the archives hold them, under any loose replacement. No discovery, no loose files.
pub struct Archived<'a>(pub &'a dyn Source);

impl Source for Archived<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.read_archived(path)
    }
    fn game_string(&self, name: &str) -> Option<String> {
        self.0.game_string(name)
    }
    fn game_string_is_built_in(&self, name: &str) -> bool {
        self.0.game_string_is_built_in(name)
    }
}

fn norm(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

/// A tile: its tree, its slot in the tree and the slot's generation when the tile was
/// made. An unloaded tile's slot is used again for a later tile with the next generation,
/// so an id kept past its tile's unloading names no tile ([`Ui::tile`] answers a dead,
/// empty tile) and never the slot's new tile; nor does an id kept from a tree since
/// replaced (a load builds a new one) name a tile of the new tree [design: #1039, #1061].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    tree: u32,
    index: u32,
    gen: u32,
}

/// The number the next [`Ui::new`] takes, so each tree's ids are its own.
static NEXT_TREE: AtomicU32 = AtomicU32::new(1);

/// Updates in a row that each followed a change of the tree's shape before
/// [`Ui::take_reshape_warning`] answers [design: #1061, about two seconds of frames;
/// loading a menu takes a few].
const RESHAPED_RUN_WARNING: u32 = 120;

/// Sentinel id for [`DEAD`] tiles in `const` contexts [design].
const DEAD_ID: TileId = TileId { tree: 0, index: 0, gen: 0 };

/// Deterministic case-insensitive 64-bit FNV-1a hash of a tile name for scoped
/// structural invalidation [design]. A hash collision can only trigger an extra
/// re-evaluation, never a missed invalidation.
fn name_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b.to_ascii_lowercase());
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn bump_name_version(versions: &mut HashMap<u64, u64>, key: u64) {
    *versions.entry(key).or_insert(0) += 1;
}

/// Structural equality on XML node slices ignoring source line numbers; top-level
/// whitespace-only text nodes in `<template>` bodies are ignored because [`Ui::fill`]
/// skips non-element nodes [design].
fn nodes_same(a: &[Node], b: &[Node], ignore_ws_text: bool) -> bool {
    let keep = |n: &&Node| !(ignore_ws_text && matches!(n, Node::Text(t) if t.trim().is_empty()));
    let mut ia = a.iter().filter(keep);
    let mut ib = b.iter().filter(keep);
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return true,
            (Some(Node::Text(tx)), Some(Node::Text(ty))) if tx == ty => {}
            (Some(Node::Entity(ex)), Some(Node::Entity(ey))) if ex == ey => {}
            (Some(Node::Element(ex)), Some(Node::Element(ey)))
                if ex.name == ey.name && ex.attrs == ey.attrs && nodes_same(&ex.children, &ey.children, ignore_ws_text && is_tile(ex)) => {}
            _ => return false,
        }
    }
}

/// A structural resolution condition relied on when evaluating an expression [design].
/// While every recorded condition and every recorded trait source still holds, re-running
/// [`Ui::eval_reading`] resolves the same tiles and traits and produces the same value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructDep {
    /// `Src::Child(_)`, `Src::Sibling(_)` under a parent, or bare `child`/`sibling`
    /// fallback: valid while `tile` is live and `tile.children_changed == seen`.
    Children { tile: TileId, seen: u64 },
    /// Structural `childcount` fallback on `tile` (no explicit `childcount` trait):
    /// valid while `tile` is live, has no explicit `childcount` trait, and
    /// `tile.children_changed <= tr.evaluated`.
    ChildCount { tile: TileId },
    /// Trait lookup on `tile` returned `None`: valid while `tile` is live and
    /// `tile.traits_shape == seen_traits`.
    MissingTrait { tile: TileId, seen_traits: u64 },
    /// `Src::Sibling(Some(name))` on a menu root, or `Src::Named(name)` that reached
    /// `root_named(name)`: valid while `root_name_version(key) == seen`.
    RootName { key: u64, seen: u64 },
    /// `Src::Named(name)` inside `root`: valid while `root` is live and
    /// `descendant_name_version(root, key) == seen`.
    DescendantName { root: TileId, key: u64, seen: u64 },
}

impl TileId {
    /// The tile's slot: the same for a later tile made there, so not an identity.
    pub fn index(self) -> u32 {
        self.index
    }
}

/// [design] Creator of a tile; descendants retain that creator across includes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// [design] Path passed to Source, and its loose-file status when loaded.
    File {
        path: String,
        loose: bool,
    },
    Injected,
    Template,
    Code,
}

impl Origin {
    fn file(src: &dyn Source, path: &str) -> Self {
        Self::File { path: path.to_string(), loose: src.is_loose(path) }
    }

    fn mod_added(&self) -> bool {
        matches!(self, Self::Injected | Self::Template | Self::File { loose: true, .. })
    }
}

/// An external write's opaque identity within its Ui, not writable authority.
/// Pair with the full TileId and trait name, never a value or vector index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraitWriteSerial(NonZeroU64);

/// One existing, non-expression trait write, conditional on its current external-write token.
/// A batch publishes all sinks together or leaves every sink unchanged [design].
#[derive(Clone, Debug)]
pub struct ConditionalTraitWrite<'a> {
    pub tile: TileId,
    pub name: &'a str,
    pub expected: TraitWriteSerial,
    pub value: Value,
}

/// One trait of a tile.
#[derive(Clone, Debug)]
pub struct Trait {
    /// As the file writes it.
    pub name: String,
    pub value: Value,
    /// The operators that work the value out, when it is not a plain value.
    pub expr: Option<Vec<Op>>,
    /// The tree's change count when the value last changed.
    pub changed: u64,
    /// Every external setter/XML replacement, even equal values; never expression work.
    write_serial: Option<TraitWriteSerial>,
    /// The change count when the expression was last worked out (0: never).
    evaluated: u64,
    /// [design] The lower-case setting whose built-in label this trait holds, as the file
    /// wrote it (`&-sName;` alone, no plugin defining the name); none once anything writes it.
    /// [`Ui::built_in_label`].
    pub built_in_label: Option<String>,
    /// The traits (tile, index) the expression read when last worked out, together with
    /// the structural conditions relied on: while all of them still hold and none of the
    /// trait sources changed since `evaluated`, [`Ui::update`] skips it without working it
    /// out [design: #1039].
    sources: Vec<(TileId, u32)>,
    struct_deps: Vec<StructDep>,
    seen_valid: bool,
    seen_root_shape: u64,
    seen_roots_shape: u64,
}

impl Trait {
    fn new(name: String, value: Value, expr: Option<Vec<Op>>, changed: u64) -> Trait {
        Trait {
            name,
            value,
            expr,
            changed,
            write_serial: None,
            evaluated: 0,
            built_in_label: None,
            sources: Vec::new(),
            struct_deps: Vec::new(),
            seen_valid: false,
            seen_root_shape: 0,
            seen_roots_shape: 0,
        }
    }
}

/// A `<template>`: content kept to make tiles from later (`AddTileFromTemplate`, the
/// game's code filling lists).
#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug)]
pub struct Tile {
    /// [design] Provenance of this tile, independent of later trait writes.
    pub origin: Origin,
    /// The tag in lower case: one of [`TILE_KINDS`].
    pub kind: String,
    pub name: String,
    pub parent: Option<TileId>,
    /// The root menu (or pseudo-tile) owning this tile, fixed at creation [design].
    root: TileId,
    pub children: Vec<TileId>,
    /// Value-generation dependency for an expression's structural childcount fallback and
    /// `child()`/`sibling()` resolution.
    children_changed: u64,
    /// Bumped when a trait is added to or removed from this tile (not on in-place value
    /// or expression updates) [design].
    traits_shape: u64,
    /// True when this tile carries an explicit `childcount` trait, superseding the
    /// structural `children.len()` fallback [design].
    explicit_childcount: bool,
    /// On a root tile: bumped when any descendant tile is added or unloaded;
    /// used as a fast-path guard before consulting per-name structural versions [design].
    root_shape: u64,
    /// On a root tile: true once at least one expression under this root has recorded
    /// structural dependencies during [`Ui::update`] [design].
    root_evaluated: bool,
    pub traits: Vec<Trait>,
    /// Case-insensitive FNV-1a hashes parallel to `traits` so [`Tile::trait_index`]
    /// scans packed `u64`s instead of chasing `Trait`/`String` pointers [design].
    trait_hashes: Vec<u64>,
    /// Indices into `traits` that carry an expression (`expr.is_some()`), kept sorted
    /// ascending so [`Ui::update`] skips plain value traits [design].
    expr_traits: Vec<u32>,
    pub templates: Vec<Template>,
    /// False once unloaded: [`Ui::tile`] answers such a tile for an id kept past its
    /// tile's unloading.
    pub live: bool,
}

/// What [`Ui::tile`] answers for an unloaded tile's id.
static DEAD: Tile = Tile::dead();

impl Tile {
    const fn dead() -> Tile {
        Tile {
            origin: Origin::Code,
            kind: String::new(),
            name: String::new(),
            parent: None,
            root: DEAD_ID,
            children: Vec::new(),
            children_changed: 0,
            traits_shape: 0,
            explicit_childcount: false,
            root_shape: 0,
            root_evaluated: false,
            traits: Vec::new(),
            trait_hashes: Vec::new(),
            expr_traits: Vec::new(),
            templates: Vec::new(),
            live: false,
        }
    }

    fn mark_expr_trait(&mut self, k: u32) {
        if let Err(pos) = self.expr_traits.binary_search(&k) {
            self.expr_traits.insert(pos, k);
        }
    }

    fn unmark_expr_trait(&mut self, k: u32) {
        if let Ok(pos) = self.expr_traits.binary_search(&k) {
            self.expr_traits.remove(pos);
        }
    }

    #[inline]
    pub fn trait_index(&self, name: &str) -> Option<usize> {
        let key = name_key(name);
        for (i, &h) in self.trait_hashes.iter().enumerate() {
            if h == key && self.traits[i].name.eq_ignore_ascii_case(name) {
                return Some(i);
            }
        }
        None
    }

    /// A trait's value by name (any case).
    #[inline]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.trait_index(name).map(|k| &self.traits[k].value)
    }
}

/// How a path's `name:N` / `*:N` counts the children that match (`Ui::find_trait_counted`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    /// From the last child, from 0: JIP's functions (GetUIFloatAlt and the rest) [observed:
    /// private jobs hud_hardcore hc0, hud_xp_limbs, hud_prompt2 p0-p8; hud.md §14.8].
    FromLast,
    /// From the first child, from 1, with 0 the last and nothing past the one before the
    /// last: the extender's GetUIFloat, SetUIFloat, SetUIString and SetUIStringEx
    /// [observed: GetUIFloat under the pause column, `*:1` its first child, `*:0` its last,
    /// `*:8` of 8 nothing, menu-xml.md §10.1; documented: MCM Guide v6, Introduction and The
    /// Options Panel: `StartMenu/MCM/*:4` is MCM's fourth child, `*:1/*:2` the options
    /// panel's Option2, `*:9` MCM_Info, the ninth; menu-xml.md §10.4].
    FromFirst,
}

/// The menus' tile tree.
#[derive(Default)]
pub struct Ui {
    /// [design] Evaluated list entries and cumulative outcomes for this tree's lifetime.
    pub uio: crate::uio::Report,
    uio_tiles: HashSet<TileId>,
    /// The tiles by slot; an unloaded tile's slot holds a dead tile until it is used again.
    tiles: Vec<Tile>,
    /// Each slot's generation, one up each time its tile is unloaded.
    slot_gens: Vec<u32>,
    /// The slots of unloaded tiles, to use again [design: #1039, so opening and closing a
    /// menu leaves the tree its size], the lowest first: a menu's tiles then take slots in
    /// the order they are made, parents before children, as [`Ui::update`] walks the
    /// slots, so a reopened menu settles in as few passes as when first opened.
    free: BinaryHeap<Reverse<u32>>,
    /// One up whenever tiles are made, unloaded, renamed, or given/stripped of traits or
    /// templates [design].
    shape: u64,
    /// Bumped whenever a root tile is added, unloaded, or renamed; fast-path guard for
    /// cross-menu [`StructDep::RootName`] checks [design].
    roots_shape: u64,
    /// Per-root-name structural versions (keyed by [`name_key`]) for cross-menu lookups
    /// (`sibling(Menu)` on a root, or `Named(Menu)` falling through to `root_named`) [design].
    root_name_versions: HashMap<u64, u64>,
    /// Per-root descendant-name structural versions (keyed by [`name_key`]), populated
    /// only after a root has evaluated expressions and removed when the root unloads [design].
    descendant_name_versions: HashMap<TileId, HashMap<u64, u64>>,
    /// First descendant of each lower-case name per root in file order, invalidated only
    /// for the root whose tiles changed [design].
    root_names: HashMap<TileId, OnceLock<HashMap<String, TileId>>>,
    /// This tree's number, in every [`TileId`] it makes ([`Ui::new`]).
    tree: u32,
    /// The shape when [`Ui::update`] last ran, the updates in a row that found it changed,
    /// whether that was told ([`Ui::take_reshape_warning`]), and the last change of shape
    /// made from outside the tree: what did it and from where.
    shape_at_update: u64,
    reshaped_run: u32,
    reshape_told: bool,
    reshaped_by: Option<(&'static str, &'static Location<'static>)>,
    /// The public call under way and where it was called from, for `reshaped_by`.
    op: Option<(&'static str, &'static Location<'static>)>,
    /// The loaded menus (and `globals()`, `screen()`, `strings()`), in load order.
    roots: Vec<TileId>,
    globals: Option<TileId>,
    screen: Option<TileId>,
    strings: Option<TileId>,
    /// Counts value changes: a trait whose sources changed after it was last worked out
    /// is worked out again.
    gen: u64,
    /// A tile, a file trait or a value changed since [`Ui::update`] last finished: with
    /// none, it has nothing to work out [design: writes of the same value, as the
    /// engine's every frame, leave the tree clean].
    dirty: bool,
    /// Trait writes from outside so far, and those that changed the value
    /// ([`Ui::trait_write_counts`]). Not behind the test aid: an integer added to in
    /// `set_at`, as `gen` is beside them, with no lock, allocation or output; a gate would
    /// read a flag in the same place.
    trait_writes: u64,
    trait_changes: u64,
    /// Ui-local external-write clock; exhausted writes are unownable, never wrapped.
    last_write_serial: u64,
    /// How many live mod-added tiles (an `Injected`, `Template` or loose file origin, or
    /// a loose UIO include's) each root tile holds, itself included, by root: kept where
    /// tiles are made, unloaded or change origin, so [`Ui::has_mod_added_tiles`] walks
    /// nothing [design: maintainer's review of #902, #938].
    mod_tiles: HashMap<TileId, usize>,
    /// What loading mended or left out, one line each.
    pub warnings: Vec<String>,
    /// The game strings (`&-sName;`) read into traits so far, by lower-case name, and
    /// whether the source had text for it (menu-xml.md §2.4) [design].
    pub game_strings_read: BTreeMap<String, bool>,
    /// What the game's own code last wrote to a tile's trait, for the writes it makes only
    /// when its value changes, so a script's value in between lasts (`Ui::write_changed`).
    code_written: HashMap<(TileId, String), Value>,
    /// Expressions [`Ui::update`] worked out (not skipped), for the tests.
    #[cfg(test)]
    pub(crate) evaluations: usize,
    /// Work every expression out at every pass as before #1039, for the tests to compare.
    #[cfg(test)]
    pub(crate) never_skip: bool,
    /// Passes the last [`Ui::update`] made, for the tests.
    #[cfg(test)]
    pub(crate) passes: usize,
    /// Nodes walked to resolve named expression sources, for work-bound controls.
    #[cfg(test)]
    pub(crate) named_source_visits: std::sync::atomic::AtomicUsize,
}

impl Ui {
    /// An empty tree with the three pseudo-tiles `screen()`, `strings()` and (until
    /// [`Ui::load_globals`]) an empty `globals()`.
    pub fn new() -> Ui {
        let mut ui = Ui { tree: NEXT_TREE.fetch_add(1, Ordering::Relaxed), ..Ui::default() };
        let screen = ui.new_tile("rect", "screen()", None, Origin::Code);
        let strings = ui.new_tile("rect", "strings()", None, Origin::Code);
        let globals = ui.new_tile("rect", "globals()", None, Origin::Code);
        ui.roots.extend([screen, strings, globals]);
        ui.screen = Some(screen);
        ui.strings = Some(strings);
        ui.globals = Some(globals);
        ui
    }

    /// Sets a trait as the game's code does for a value it writes only when the value
    /// changes: nothing when `v` is what the code wrote last, whatever a script set since.
    #[track_caller]
    pub fn write_changed(&mut self, tile: TileId, name: &str, v: Value) {
        self.entered("write_changed");
        if !self.is_live(tile) {
            return;
        }
        let key = (tile, name.to_string());
        if self.code_written.get(&key) == Some(&v) {
            return;
        }
        self.set_trait(tile, name, v.clone());
        self.code_written.insert(key, v);
    }

    /// The tile; for an id kept past its tile's unloading a dead tile (not `live`, no
    /// name, children or traits).
    pub fn tile(&self, id: TileId) -> &Tile {
        match self.slot(id) {
            Some(i) => &self.tiles[i],
            None => &DEAD,
        }
    }

    /// Whether `id` names a loaded tile of this tree: false for one unloaded (even when
    /// a later tile has its slot), and for an id kept from a tree since replaced (a load
    /// builds a new one) [design].
    pub fn is_live(&self, id: TileId) -> bool {
        self.slot(id).is_some()
    }

    /// The id of the tile in slot `i` now.
    fn id_at(&self, i: usize) -> TileId {
        TileId { tree: self.tree, index: i as u32, gen: self.slot_gens[i] }
    }

    /// The id's slot, when its tile is loaded.
    fn slot(&self, id: TileId) -> Option<usize> {
        let i = id.index as usize;
        (id.tree == self.tree && self.slot_gens.get(i) == Some(&id.gen) && self.tiles[i].live).then_some(i)
    }

    /// How many slots the tree holds: the most tiles loaded at once, the pseudo-tiles
    /// included.
    pub fn tile_slots(&self) -> usize {
        self.tiles.len()
    }

    /// How many tiles are loaded, the pseudo-tiles included.
    pub fn live_tiles(&self) -> usize {
        self.tiles.len() - self.free.len()
    }

    /// A count that goes up whenever tiles are made, unloaded or changed other than in a value:
    /// what follows from the tree's shape alone stands while this does.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    /// A public call that may change the shape starts: what it is and where it was called
    /// from, kept for [`Ui::take_reshape_warning`].
    #[track_caller]
    fn entered(&mut self, what: &'static str) {
        self.op = Some((what, Location::caller()));
    }

    /// Records a real structural shape change (tile/trait/template addition, removal, or
    /// tile rename) without invalidating unrelated menus [design].
    fn bump_shape(&mut self) {
        self.shape += 1;
        self.reshaped_by = self.op;
    }

    fn root_name_version(&self, key: u64) -> u64 {
        self.root_name_versions.get(&key).copied().unwrap_or(0)
    }

    fn descendant_name_version(&self, root: TileId, key: u64) -> u64 {
        self.descendant_name_versions.get(&root).and_then(|m| m.get(&key)).copied().unwrap_or(0)
    }

    /// Invalidates `root`'s cached name map and bumps structural versions for the mutated
    /// descendant name keys inside `root` [design].
    fn invalidate_root_descendants(&mut self, root: TileId, name_keys: &[u64]) {
        if let Some(cell) = self.root_names.get_mut(&root) {
            cell.take();
        }
        if let Some(r_slot) = self.slot(root) {
            self.tiles[r_slot].root_shape += 1;
            if self.tiles[r_slot].root_evaluated && !name_keys.is_empty() {
                let versions = self.descendant_name_versions.entry(root).or_default();
                for &key in name_keys {
                    bump_name_version(versions, key);
                }
            }
        }
    }

    /// [design: #1061] Once, when the shape changed before each of the last
    /// [`RESHAPED_RUN_WARNING`] updates: a line naming the last call that changed it and
    /// where from. The engine asks after each update and logs it.
    pub fn take_reshape_warning(&mut self) -> Option<String> {
        if self.reshape_told || self.reshaped_run < RESHAPED_RUN_WARNING {
            return None;
        }
        self.reshape_told = true;
        let by = match self.reshaped_by {
            Some((what, at)) => format!("{what} called at {}:{}", at.file(), at.line()),
            None => "unknown".into(),
        };
        Some(format!("ui tree: its shape changed before each of {} updates in a row; last change: {by}", self.reshaped_run))
    }

    pub fn screen(&self) -> TileId {
        self.screen.expect("Ui::new makes screen()")
    }

    pub fn strings(&self) -> TileId {
        self.strings.expect("Ui::new makes strings()")
    }

    pub fn globals(&self) -> TileId {
        self.globals.expect("Ui::new makes globals()")
    }

    /// The live menus' root tiles (not the pseudo-tiles).
    pub fn menus(&self) -> impl Iterator<Item = TileId> + '_ {
        self.roots.iter().copied().filter(|&r| self.tile(r).kind == "menu")
    }

    /// The loaded menu of that name (any case).
    pub fn menu(&self, name: &str) -> Option<TileId> {
        self.menus().find(|&r| self.tile(r).name.eq_ignore_ascii_case(name))
    }

    fn new_tile(&mut self, kind: &str, name: &str, parent: Option<TileId>, origin: Origin) -> TileId {
        self.dirty = true;
        self.bump_shape();
        let tile = Tile { origin, kind: kind.to_ascii_lowercase(), name: name.to_string(), parent, live: true, ..Tile::dead() };
        let id = match self.free.pop() {
            Some(Reverse(index)) => {
                self.tiles[index as usize] = tile;
                self.id_at(index as usize)
            }
            None => {
                self.tiles.push(tile);
                self.slot_gens.push(0);
                self.id_at(self.tiles.len() - 1)
            }
        };
        if let Some(p) = parent {
            let p_slot = self.slot(p).expect("a loaded parent tile");
            let root = self.tiles[p_slot].root;
            self.tiles[id.index as usize].root = root;
            self.gen += 1;
            let changed = self.gen;
            self.tiles[p_slot].children.push(id);
            self.tiles[p_slot].children_changed = changed;
            self.invalidate_root_descendants(root, &[name_key(name)]);
        } else {
            self.tiles[id.index as usize].root = id;
            self.tiles[id.index as usize].root_shape = 1;
            self.root_names.insert(id, OnceLock::new());
            self.roots_shape += 1;
            bump_name_version(&mut self.root_name_versions, name_key(name));
        }
        if self.tile(id).origin.mod_added() {
            self.count_mod_tile(id, true);
        }
        id
    }

    /// Whether a tile counts in [`Ui::mod_added_tiles`]' walk as the mod's: its origin, or
    /// a loose UIO include's.
    fn is_mod_added(&self, id: TileId) -> bool {
        self.tile(id).origin.mod_added() || self.uio_tiles.contains(&id)
    }

    /// One mod-added tile more (`up`) or less under its root ([`Ui::mod_tiles`]).
    fn count_mod_tile(&mut self, id: TileId, up: bool) {
        let root = self.root_of(id);
        let n = self.mod_tiles.entry(root).or_default();
        if up {
            *n += 1;
        } else {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.mod_tiles.remove(&root);
            }
        }
    }

    /// Counts the live mod-added tiles again, after the UIO tiles were forgotten.
    fn recount_mod_tiles(&mut self) {
        self.mod_tiles.clear();
        for i in 0..self.tiles.len() {
            let id = self.id_at(i);
            if self.tile(id).live && self.is_mod_added(id) {
                self.count_mod_tile(id, true);
            }
        }
    }

    // ---- loading --------------------------------------------------------------------

    /// [design] Discover and evaluate UIO once before loading menus; the caller supplies
    /// active plugins. Rebuilt menus reuse these entries (menu-xml.md §7 item 3).
    pub fn configure_uio(&mut self, src: &dyn Source, plugin: impl Fn(&str) -> bool) {
        self.uio = crate::uio::Report::read(src, &plugin);
        self.uio_tiles.clear();
        self.recount_mod_tiles();
    }

    /// Reads `menus\globals.xml` into `globals()`.
    #[track_caller]
    pub fn load_globals(&mut self, src: &dyn Source) -> bool {
        self.entered("load_globals");
        let Some(doc) = self.read_doc(src, "menus\\globals.xml") else {
            return false;
        };
        let g = self.globals();
        let origin = Origin::file(src, "menus\\globals.xml");
        let counted = self.tile(g).live && self.is_mod_added(g);
        if let Some(slot) = self.slot(g) {
            if self.tiles[slot].origin != origin {
                self.tiles[slot].origin = origin.clone();
                self.bump_shape();
            }
        }
        if counted != (self.tile(g).live && self.is_mod_added(g)) {
            self.count_mod_tile(g, !counted);
        }
        // The file is one `<rect name="Strings">` [observed]; its content is globals()'.
        for n in &doc.nodes {
            if let Node::Element(e) = n {
                if is_tile(e) {
                    self.fill(src, g, &e.children, 0, &origin);
                } else {
                    self.fill(src, g, std::slice::from_ref(n), 0, &origin);
                }
            }
        }
        true
    }

    /// Reads a menu file (`menus\…\x_menu.xml`) and builds its menu; a menu of the same
    /// name already loaded is unloaded first. Returns the menu's tile.
    #[track_caller]
    pub fn load_menu(&mut self, src: &dyn Source, path: &str) -> Result<TileId, String> {
        self.entered("load_menu");
        let doc = self.read_doc(src, path).ok_or_else(|| format!("{path}: not found"))?;
        let menu = doc
            .nodes
            .iter()
            .find_map(|n| match n {
                Node::Element(e) if e.is("menu") => Some(e),
                _ => None,
            })
            .ok_or_else(|| format!("{path}: no <menu> element"))?;
        let name = menu.attr("name").unwrap_or("").to_string();
        if let Some(old) = self.menu(&name) {
            self.unload_tile(old);
        }
        let origin = Origin::file(src, path);
        let id = self.new_tile("menu", &name, None, origin.clone());
        self.roots.push(id);
        self.fill(src, id, &menu.children, 0, &origin);
        let entries: Vec<_> = self
            .uio
            .decisions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.enabled && matches!(&d.entry.target, crate::uio::Target::Menu(n) if n.eq_ignore_ascii_case(&name)))
            .map(|(i, d)| (i, d.entry.clone()))
            .collect();
        for (i, entry) in entries {
            let target = match entry.tile {
                Some(ref name) => self.descendant_named(id, name),
                None => Some(id),
            };
            if let Some(target) = target {
                // [hypothesis] Append after existing content: preserves file order and
                // lets list traits override earlier traits; UIO's position is unmeasured.
                let node = uio_include(&entry.include);
                self.fill(src, target, &[node], 0, &origin);
                self.uio.decisions[i].applied += 1;
                self.uio.decisions[i].reason = "applied".into();
            }
        }
        Ok(id)
    }

    fn descendant_named(&self, root: TileId, name: &str) -> Option<TileId> {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if self.tile(id).name.eq_ignore_ascii_case(name) {
                return Some(id);
            }
            stack.extend(self.tile(id).children.iter().rev().copied());
        }
        None
    }

    fn read_doc(&mut self, src: &dyn Source, path: &str) -> Option<xml::Doc> {
        let bytes = src.read(path)?;
        let mut doc = xml::parse(&xml::decode(&bytes));
        for w in &doc.warnings {
            self.warnings.push(format!("{path}: {w}"));
        }
        // [design] Patch prefab syntax before fill/expansion, including named tiles in
        // template bodies that have no TileId until a template is instantiated.
        for decision in &mut self.uio.decisions {
            if !decision.enabled || !matches!(&decision.entry.target, crate::uio::Target::Prefab(p) if *p == norm(path)) {
                continue;
            }
            let target = match &decision.entry.tile {
                Some(name) => named_nodes(&mut doc.nodes, name),
                None => untargeted_nodes(&mut doc.nodes),
            };
            if let Some(nodes) = target {
                nodes.push(uio_include(&decision.entry.include));
                decision.applied += 1;
                decision.reason = "applied".into();
            }
        }
        Some(doc)
    }

    /// Puts content into a tile: child tiles, templates, traits, and the content of
    /// included files where they are included.
    fn fill(&mut self, src: &dyn Source, tile: TileId, nodes: &[Node], depth: usize, origin: &Origin) {
        for n in nodes {
            let Node::Element(e) = n else { continue };
            if is_tile(e) {
                let child = self.new_tile(&e.name, e.attr("name").unwrap_or(""), Some(tile), origin.clone());
                self.fill(src, child, &e.children, depth, origin);
            } else if e.is("template") {
                let name = e.attr("name").unwrap_or("").to_string();
                let body = self.expand_includes(src, &e.children, depth);
                let slot = self.slot(tile).expect("a loaded tile");
                // Re-injecting an identical template is a no-op [design].
                if !self.tiles[slot].templates.iter().any(|tp| tp.name.eq_ignore_ascii_case(&name) && nodes_same(&tp.body, &body, true)) {
                    self.tiles[slot].templates.push(Template { name, body });
                    self.bump_shape();
                }
            } else if e.is("include") {
                let file = e.attr("src").unwrap_or("");
                if depth >= MAX_INCLUDE_DEPTH {
                    self.warnings.push(format!("include {file}: nested too deep; left out"));
                    continue;
                }
                let path = prefab_path(file);
                let included_origin = match origin {
                    Origin::File { .. } => Origin::file(src, &path),
                    other => other.clone(),
                };
                match self.read_doc(src, &path) {
                    Some(doc) => {
                        let start = self.tile(tile).children.len();
                        self.fill(src, tile, &doc.nodes, depth + 1, &included_origin);
                        // [design] Track this inclusion's tiles, rather than all uses of
                        // its filename: the same prefab may also occur in vanilla content.
                        // They are the children it added to `tile` and theirs (slots are
                        // used again, so they are no run of slots).
                        if e.attr("nvf_uio") == Some("true") && src.is_loose(&path) {
                            let mut stack = self.tile(tile).children[start..].to_vec();
                            while let Some(id) = stack.pop() {
                                stack.extend(self.tile(id).children.iter().copied());
                                let counted = self.is_mod_added(id);
                                if self.uio_tiles.insert(id) && !counted && self.tile(id).live {
                                    self.count_mod_tile(id, true);
                                }
                            }
                        }
                    }
                    None => self.warnings.push(format!("include {file}: not found")),
                }
            } else {
                self.put_trait(src, tile, e);
            }
        }
    }

    /// A template's content with its includes spliced in, so that tiles made from it
    /// later need no files.
    fn expand_includes(&mut self, src: &dyn Source, nodes: &[Node], depth: usize) -> Vec<Node> {
        let mut out = Vec::new();
        for n in nodes {
            match n {
                Node::Element(e) if e.is("include") && depth < MAX_INCLUDE_DEPTH => {
                    let file = e.attr("src").unwrap_or("");
                    match self.read_doc(src, &prefab_path(file)) {
                        Some(doc) => out.extend(self.expand_includes(src, &doc.nodes, depth + 1)),
                        None => self.warnings.push(format!("include {file}: not found")),
                    }
                }
                Node::Element(e) => {
                    let mut e = e.clone();
                    e.children = self.expand_includes(src, &e.children, depth);
                    out.push(Node::Element(e));
                }
                other => out.push(other.clone()),
            }
        }
        out
    }

    /// A trait element: a later one of the same name replaces the earlier [hypothesis:
    /// how the original treats a trait written twice is not known; includes that set a
    /// trait the including file set too depend on it].
    fn put_trait(&mut self, src: &dyn Source, tile: TileId, e: &Element) {
        let asked = RefCell::new(Vec::new());
        let string = |n: &str, explicit: bool| {
            let text = src.game_string(n);
            if explicit || text.is_some() {
                asked.borrow_mut().push((n.to_ascii_lowercase(), text.is_some()));
            }
            text
        };
        let mut warns = Vec::new();
        let (value, expr) = if expr::has_ops(e) {
            (Value::default(), Some(expr::ops_of_with(&e.children, &string, &mut |w| warns.push(w))))
        } else {
            (expr::literal_with(&e.children, &string), None)
        };
        let asked = asked.into_inner();
        let built_in_label = match asked.as_slice() {
            [(name, true)] if expr.is_none() && src.game_string_is_built_in(name) && src.game_string(name).is_some_and(|t| t.trim() == value.text().trim()) => Some(name.clone()),
            _ => None,
        };
        for (name, found) in asked {
            *self.game_strings_read.entry(name).or_insert(false) |= found;
        }
        for w in warns {
            self.warnings.push(format!("<{}>: {w}", e.name));
        }
        let existing = self.tile(tile).trait_index(&e.name);
        let write_serial = self.next_write_serial();
        let slot = self.slot(tile).expect("a loaded tile");
        match existing {
            // Updating an existing trait slot in-place does not change tile hierarchy,
            // names, or trait existence/indices, so it avoids reshaping the tree [design].
            Some(k) => match expr {
                Some(ops) => {
                    self.gen += 1;
                    let gen = self.gen;
                    let old = &mut self.tiles[slot].traits[k];
                    old.name = e.name.clone();
                    old.value = Value::default();
                    old.expr = Some(ops);
                    old.changed = gen;
                    old.write_serial = write_serial;
                    old.evaluated = 0;
                    old.built_in_label = built_in_label;
                    old.sources.clear();
                    old.struct_deps.clear();
                    old.seen_valid = false;
                    self.tiles[slot].mark_expr_trait(k as u32);
                    self.dirty = true;
                }
                None => {
                    let differs = value.differs(&self.tiles[slot].traits[k].value);
                    let old = &mut self.tiles[slot].traits[k];
                    old.name = e.name.clone();
                    old.value = value;
                    old.expr = None;
                    old.write_serial = write_serial;
                    old.evaluated = 0;
                    old.built_in_label = built_in_label;
                    old.sources.clear();
                    old.struct_deps.clear();
                    old.seen_valid = false;
                    self.tiles[slot].unmark_expr_trait(k as u32);
                    if differs {
                        self.gen += 1;
                        let gen = self.gen;
                        self.tiles[slot].traits[k].changed = gen;
                        self.dirty = true;
                    }
                }
            },
            None => {
                self.gen += 1;
                let gen = self.gen;
                let key = name_key(&e.name);
                let has_expr = expr.is_some();
                let mut new = Trait::new(e.name.clone(), value, expr, gen);
                new.write_serial = write_serial;
                new.built_in_label = built_in_label;
                let idx = self.tiles[slot].traits.len() as u32;
                self.tiles[slot].traits.push(new);
                self.tiles[slot].trait_hashes.push(key);
                if has_expr {
                    self.tiles[slot].expr_traits.push(idx);
                }
                self.tiles[slot].traits_shape += 1;
                if e.name.eq_ignore_ascii_case("childcount") {
                    self.tiles[slot].explicit_childcount = true;
                }
                self.dirty = true;
                self.bump_shape();
            }
        }
    }

    /// Unloads a tile and everything in it, and frees their slots for later tiles;
    /// nothing for an id whose tile is already unloaded.
    #[track_caller]
    pub fn unload_tile(&mut self, id: TileId) {
        self.entered("unload_tile");
        if !self.is_live(id) {
            return;
        }
        let root = self.root_of(id);
        let parent = self.tile(id).parent;
        let root_key = name_key(&self.tile(id).name);
        // Counted off while the tiles are all there: a tile's root is found through them.
        let mut stack = vec![id];
        let mut gone = Vec::new();
        let mut gone_name_keys = Vec::new();
        while let Some(t) = stack.pop() {
            if self.is_mod_added(t) {
                self.count_mod_tile(t, false);
            }
            if parent.is_some() {
                gone_name_keys.push(name_key(&self.tile(t).name));
            }
            stack.extend(self.tile(t).children.iter().copied());
            gone.push(t);
        }
        if let Some(p) = parent {
            self.gen += 1;
            let changed = self.gen;
            if let Some(p_slot) = self.slot(p) {
                self.tiles[p_slot].children.retain(|&c| c != id);
                self.tiles[p_slot].children_changed = changed;
            }
            self.invalidate_root_descendants(root, &gone_name_keys);
        } else {
            self.roots.retain(|&r| r != id);
            self.root_names.remove(&id);
            self.descendant_name_versions.remove(&id);
            self.roots_shape += 1;
            bump_name_version(&mut self.root_name_versions, root_key);
        }
        self.dirty = true;
        self.bump_shape();
        for &t in &gone {
            let i = t.index as usize;
            self.tiles[i] = Tile::dead();
            self.slot_gens[i] = self.slot_gens[i].wrapping_add(1);
            self.free.push(Reverse(t.index));
        }
        // Forget what was kept by the tiles' ids, so nothing grows as menus open and close.
        if !self.uio_tiles.is_empty() || !self.code_written.is_empty() {
            let gone: HashSet<TileId> = gone.into_iter().collect();
            self.uio_tiles.retain(|t| !gone.contains(t));
            self.code_written.retain(|(t, _), _| !gone.contains(t));
        }
        self.gen += 1;
    }

    // ---- paths ----------------------------------------------------------------------

    /// The tile a path names: `Menu/Tile/…/Tile`, with `/` or `\`, names in any case, `*`
    /// for any child and `name:N` (or `*:N`) for the N-th (from 0) of the children that
    /// match, counting from the last child [observed: private jobs hud_hardcore hc0,
    /// hud_xp_limbs all, hud_prompt2 p0-p8; menu-xml.md §5]. Unindexed lookup keeps
    /// file order [hypothesis: menu-xml.md §5].
    pub fn find_tile(&self, path: &str) -> Option<TileId> {
        let mut stack = [""; 16];
        let mut heap = Vec::new();
        let segs = split_slice(path, &mut stack, &mut heap);
        let (first, rest) = segs.split_first()?;
        let root = self.root_named(first)?;
        self.walk(root, rest, Count::FromLast)
    }

    /// Resolve a relative child path under an already-known `parent` tile (zero allocation)
    /// [design].
    pub fn find_child_path(&self, parent: TileId, rel: &str) -> Option<TileId> {
        let mut stack = [""; 16];
        let mut heap = Vec::new();
        let segs = split_slice(rel, &mut stack, &mut heap);
        self.walk(parent, segs, Count::FromLast)
    }

    /// The tile and trait a path names: [`Ui::find_tile`] of all but the last segment,
    /// and the last a trait of it.
    pub fn find_trait(&self, path: &str) -> Option<(TileId, usize)> {
        self.find_trait_counted(path, Count::FromLast)
    }

    /// [`Ui::find_trait`] with `name:N` / `*:N` counted as `count` says (the extender's
    /// GetUIFloat and SetUIFloat count from the first child, [`Count::FromFirst`]).
    pub fn find_trait_counted(&self, path: &str, count: Count) -> Option<(TileId, usize)> {
        let mut stack = [""; 16];
        let mut heap = Vec::new();
        let segs = split_slice(path, &mut stack, &mut heap);
        let (name, tiles) = segs.split_last()?;
        let (first, rest) = tiles.split_first()?;
        let root = self.root_named(first)?;
        self.walk_to_trait(root, rest, name, count)
    }

    /// A trait's value by a path counted as `count` says ([`Ui::find_trait_counted`]).
    pub fn get_counted(&self, path: &str, count: Count) -> Option<&Value> {
        let (t, k) = self.find_trait_counted(path, count)?;
        Some(&self.tile(t).traits[k].value)
    }

    /// Sets a trait by a path counted as `count` says, as [`Ui::set`] does.
    pub fn set_counted(&mut self, path: &str, count: Count, v: Value) -> bool {
        let Some((t, k)) = self.find_trait_counted(path, count) else { return false };
        self.set_at(t, k, v);
        true
    }

    /// A trait by a path under a tile: `Tile/…/trait` from `at`'s children, or a trait of
    /// `at` itself when the path is one segment (SortUIListBox's keys, read on each item)
    /// [design].
    pub fn find_trait_in(&self, at: TileId, path: &str) -> Option<(TileId, usize)> {
        let mut stack = [""; 16];
        let mut heap = Vec::new();
        let segs = split_slice(path, &mut stack, &mut heap);
        let (name, tiles) = segs.split_last()?;
        self.walk_to_trait(at, tiles, name, Count::FromLast)
    }

    fn root_named(&self, seg: &str) -> Option<TileId> {
        self.roots.iter().copied().find(|&r| self.tile(r).live && self.tile(r).name.eq_ignore_ascii_case(seg))
    }

    /// The single child of `at` matching an indexed segment (`name:k` or `*:k`), without
    /// allocating an intermediate `Vec`.
    fn find_indexed_child(&self, at: TileId, name: &str, k: usize, count: Count) -> Option<TileId> {
        let children = &self.tile(at).children;
        let matches = |&c: &TileId| name == "*" || self.tile(c).name.eq_ignore_ascii_case(name);
        match (k, count) {
            (k, Count::FromLast) => children.iter().rev().copied().filter(matches).nth(k),
            // 0 the last, 1 the first, N the N-th, none past the one before the last.
            (0, Count::FromFirst) => children.iter().rev().copied().find(matches),
            (k, Count::FromFirst) => {
                let mut it = children.iter().copied().filter(matches);
                let pick = it.nth(k - 1)?;
                it.next().map(|_| pick)
            }
        }
    }

    fn walk(&self, at: TileId, segs: &[&str], count: Count) -> Option<TileId> {
        let Some((seg, rest)) = segs.split_first() else { return Some(at) };
        let (name, index) = parse_seg(seg);
        if let Some(k) = index {
            return self.find_indexed_child(at, name, k, count).and_then(|c| self.walk(c, rest, count));
        }
        self.tile(at)
            .children
            .iter()
            .copied()
            .filter(|&c| name == "*" || self.tile(c).name.eq_ignore_ascii_case(name))
            .find_map(|c| self.walk(c, rest, count))
    }

    fn walk_to_trait(&self, at: TileId, segs: &[&str], trait_name: &str, count: Count) -> Option<(TileId, usize)> {
        let Some((seg, rest)) = segs.split_first() else {
            return self.tile(at).trait_index(trait_name).map(|k| (at, k));
        };
        let (name, index) = parse_seg(seg);
        if let Some(k) = index {
            return self.find_indexed_child(at, name, k, count).and_then(|c| self.walk_to_trait(c, rest, trait_name, count));
        }
        self.tile(at)
            .children
            .iter()
            .copied()
            .filter(|&c| name == "*" || self.tile(c).name.eq_ignore_ascii_case(name))
            .find_map(|c| self.walk_to_trait(c, rest, trait_name, count))
    }

    /// The path of a tile, from its menu: `HUDMainMenu/HitPoints`.
    pub fn path_of(&self, id: TileId) -> String {
        let mut names = vec![self.tile(id).name.clone()];
        let mut at = self.tile(id).parent;
        while let Some(p) = at {
            names.push(self.tile(p).name.clone());
            at = self.tile(p).parent;
        }
        names.reverse();
        names.join("/")
    }

    /// The tile's name, with `:<index>` (0-based among same-named siblings in child order)
    /// when its parent holds more than one child of that name (case-insensitively)
    /// [documented: SetOnMenuClickEventHandler, "list items get :<index>"; hypothesis:
    /// 0-based among same-named siblings].
    pub fn indexed_name_of(&self, id: TileId) -> String {
        let name = &self.tile(id).name;
        if let Some(parent) = self.tile(id).parent {
            let mut count = 0usize;
            let mut pos = 0usize;
            for &child in &self.tile(parent).children {
                if self.tile(child).name.eq_ignore_ascii_case(name) {
                    if child == id {
                        pos = count;
                    }
                    count += 1;
                }
            }
            if count > 1 {
                return format!("{name}:{pos}");
            }
        }
        name.clone()
    }

    /// The path of a tile with [`Ui::indexed_name_of`] on each segment:
    /// `BarterMenu/BarterCategories/BC_Category:1`.
    pub fn indexed_path_of(&self, id: TileId) -> String {
        let mut names = vec![self.indexed_name_of(id)];
        let mut at = self.tile(id).parent;
        while let Some(p) = at {
            names.push(self.indexed_name_of(p));
            at = self.tile(p).parent;
        }
        names.reverse();
        names.join("/")
    }

    /// The menu (or pseudo-tile) a tile is in.
    pub fn root_of(&self, id: TileId) -> TileId {
        match self.slot(id) {
            Some(i) => self.tiles[i].root,
            None => id,
        }
    }

    // ---- reading and changing traits (the UI functions' ground) ---------------------

    /// A trait's value by path: `GetUIFloat` / `GetUIString`.
    pub fn get(&self, path: &str) -> Option<&Value> {
        let (t, k) = self.find_trait(path)?;
        Some(&self.tile(t).traits[k].value)
    }

    pub fn get_float(&self, path: &str) -> Option<f64> {
        self.get(path).map(Value::num)
    }

    pub fn get_string(&self, path: &str) -> Option<String> {
        self.get(path).map(Value::text)
    }

    /// Sets a trait by path (`SetUIFloat`, `SetUIString`); false when the path names no
    /// trait [hypothesis: the setters make no trait that is not there, AddTraitToTile
    /// exists for that]. The value holds until what the trait's expression reads changes.
    pub fn set(&mut self, path: &str, v: Value) -> bool {
        let Some((t, k)) = self.find_trait(path) else { return false };
        self.set_at(t, k, v);
        true
    }

    pub fn set_float(&mut self, path: &str, v: f64) -> bool {
        self.set(path, Value::Num(v))
    }

    pub fn set_string(&mut self, path: &str, s: &str) -> bool {
        self.set(path, Value::Str(s.to_string()))
    }

    /// Sets a tile's trait by name as [`Ui::set`] does by path: false, and nothing made,
    /// when the tile has no such trait (MCM's shorthands, which find their tile by place).
    pub fn set_existing(&mut self, tile: TileId, name: &str, v: Value) -> bool {
        if !self.is_live(tile) {
            return false;
        }
        let Some(k) = self.tile(tile).trait_index(name) else { return false };
        self.set_at(tile, k, v);
        true
    }

    /// Adds to a trait by path (`ModUIFloat`).
    pub fn mod_float(&mut self, path: &str, by: f64) -> bool {
        let Some((t, k)) = self.find_trait(path) else { return false };
        let v = self.tile(t).traits[k].value.num() + by;
        self.set_at(t, k, Value::Num(v));
        true
    }

    /// Sets a tile's trait by name, making it when it is not there (the engine's own
    /// writes: the game code's `io()` traits, `screen()`'s size).
    /// Nothing for an id whose tile is unloaded.
    #[track_caller]
    pub fn set_trait(&mut self, tile: TileId, name: &str, v: Value) {
        self.entered("set_trait");
        if !self.is_live(tile) {
            return;
        }
        let k = self.trait_or_new(tile, name);
        self.set_at(tile, k, v);
    }

    /// Sets a tile's string trait without allocating a new `String` when the trait
    /// already holds the exact same text [design].
    #[track_caller]
    pub fn set_trait_str(&mut self, tile: TileId, name: &str, value: &str) {
        self.entered("set_trait");
        if !self.is_live(tile) {
            return;
        }
        let k = self.trait_or_new(tile, name);
        let unchanged = matches!(&self.tiles[tile.index as usize].traits[k].value, Value::Str(s) if s == value);
        if unchanged {
            self.gen += 1;
            self.trait_writes += 1;
            let gen = self.gen;
            let write_serial = self.next_write_serial();
            let tr = &mut self.tiles[tile.index as usize].traits[k];
            tr.write_serial = write_serial;
            tr.built_in_label = None;
            tr.evaluated = gen;
            tr.seen_valid = false;
        } else {
            self.set_at(tile, k, Value::Str(value.to_string()));
        }
    }

    /// The index of a live tile's trait by name, made (empty) when it is not there.
    fn trait_or_new(&mut self, tile: TileId, name: &str) -> usize {
        match self.tile(tile).trait_index(name) {
            Some(k) => k,
            None => {
                let slot = self.slot(tile).expect("a loaded tile");
                self.tiles[slot].traits.push(Trait::new(name.to_string(), Value::default(), None, 0));
                self.tiles[slot].trait_hashes.push(name_key(name));
                self.tiles[slot].traits_shape += 1;
                if name.eq_ignore_ascii_case("childcount") {
                    self.tiles[slot].explicit_childcount = true;
                }
                self.dirty = true;
                self.bump_shape();
                self.tiles[slot].traits.len() - 1
            }
        }
    }

    /// Gives a tile's trait the operators `ops`, making the trait when it is not there (the
    /// engine's own expression writes: an adapter that moves a mod's expression-driven place
    /// keeps the file's operators working). The value is worked out afresh on the next
    /// [`Ui::update`]; until then the trait keeps the value it had [design]. Nothing for an
    /// id whose tile is unloaded.
    #[track_caller]
    pub fn set_trait_expr(&mut self, tile: TileId, name: &str, ops: Vec<crate::expr::Op>) {
        self.entered("set_trait_expr");
        if !self.is_live(tile) {
            return;
        }
        let k = self.trait_or_new(tile, name);
        self.gen += 1;
        let gen = self.gen;
        let write_serial = self.next_write_serial();
        let slot = self.slot(tile).expect("a loaded tile");
        let tr = &mut self.tiles[slot].traits[k];
        tr.expr = Some(ops);
        tr.changed = gen;
        tr.write_serial = write_serial;
        tr.evaluated = 0;
        tr.built_in_label = None;
        tr.sources.clear();
        tr.struct_deps.clear();
        tr.seen_valid = false;
        self.tiles[slot].mark_expr_trait(k as u32);
        self.dirty = true;
    }

    /// [design] The setting whose built-in label `name` of `tile` still holds as the file wrote
    /// it: a value a plugin, a mod's file or a script chose answers none.
    pub fn built_in_label(&self, tile: TileId, name: &str) -> Option<&str> {
        let t = self.tile(tile);
        t.traits[t.trait_index(name)?].built_in_label.as_deref()
    }

    /// Trait writes from outside so far ([`Ui::set_trait`] and the like) and how many of
    /// them changed the value, for measuring what a frame writes (the engine's
    /// `NVF_PLAY_TEST_TILE_COUNTS`).
    pub fn trait_write_counts(&self) -> (u64, u64) {
        (self.trait_writes, self.trait_changes)
    }

    /// Last accepted external setter/XML insertion or replacement, including equal values.
    /// None for a missing/stale target or an exhausted clock. Expression evaluation does
    /// not advance it; an expression may have a serial but is not thereby a writable sink.
    pub fn trait_write_serial(&self, tile: TileId, name: &str) -> Option<TraitWriteSerial> {
        let t = self.tile(tile);
        t.traits.get(t.trait_index(name)?)?.write_serial
    }

    /// Atomically write 1..=8 distinct existing plain traits whose complete IDs and
    /// external-write tokens still match. Limits (4096-byte names, 65536-byte values)
    /// are engine policy. Validate all targets and counters before the first write.
    /// Success calls `set_at` directly, without `set_trait`'s reshape-diagnostic entrypoint;
    /// the batch neither creates traits nor reshapes the tree.
    pub fn set_existing_batch_if_unchanged(&mut self, writes: &[ConditionalTraitWrite<'_>]) -> Option<Vec<TraitWriteSerial>> {
        if writes.is_empty() || writes.len() > 8 { return None; }
        let count=u64::try_from(writes.len()).ok()?;
        self.last_write_serial.checked_add(count)?;
        self.gen.checked_add(count)?;
        self.trait_writes.checked_add(count)?;
        let mut bytes=0usize;
        for write in writes {
            if write.name.len()>4096 { return None; }
            bytes=bytes.checked_add(match &write.value {Value::Str(s)|Value::Sym(s)=>s.len(),Value::Num(_)=>8})?;
            if bytes>65536 { return None; }
        }
        let mut at = Vec::with_capacity(writes.len());
        for (i, write) in writes.iter().enumerate() {
            if writes[..i].iter().any(|other| other.tile == write.tile && other.name.eq_ignore_ascii_case(write.name)) { return None; }
            let tile = self.tile(write.tile);
            if !tile.live { return None; }
            let index = tile.trait_index(write.name)?;
            let tr = &tile.traits[index];
            if tr.expr.is_some() || tr.write_serial != Some(write.expected) { return None; }
            at.push((write.tile, index));
        }
        let changes=writes.iter().zip(&at).filter(|(write,(tile,index))|write.value.differs(&self.tiles[tile.index as usize].traits[*index].value)).count();
        self.trait_changes.checked_add(u64::try_from(changes).ok()?)?;
        let mut assigned = Vec::with_capacity(writes.len());
        for (write, (tile, index)) in writes.iter().zip(at) {
            self.set_at(tile, index, write.value.clone());
            assigned.push(self.trait_write_serial(tile, write.name).expect("bounded checked batch reserved serials"));
        }
        Some(assigned)
    }

    fn next_write_serial(&mut self) -> Option<TraitWriteSerial> {
        let next = self.last_write_serial.checked_add(1)?;
        self.last_write_serial = next;
        NonZeroU64::new(next).map(TraitWriteSerial)
    }

    fn set_at(&mut self, t: TileId, k: usize, v: Value) {
        self.gen += 1;
        self.trait_writes += 1;
        let gen = self.gen;
        let write_serial = self.next_write_serial();
        let tr = &mut self.tiles[t.index as usize].traits[k];
        tr.write_serial = write_serial;
        if v.differs(&tr.value) {
            tr.changed = gen;
            self.dirty = true;
            self.trait_changes += 1;
        }
        tr.value = v;
        tr.built_in_label = None;
        // A trait set from outside keeps its value until a source changes again. What it
        // read last may have changed before this write, so the next update works it out
        // to learn its sources again.
        tr.evaluated = gen;
        tr.seen_valid = false;
    }

    /// Takes a trait off a tile, as if it had never had it (an engine's own write undone
    /// [design]): what reads it reads nothing again. False when the tile has none.
    #[track_caller]
    pub fn remove_trait(&mut self, tile: TileId, name: &str) -> bool {
        self.entered("remove_trait");
        if !self.is_live(tile) {
            return false;
        }
        let Some(k) = self.tile(tile).trait_index(name) else { return false };
        // Whoever read it last works its value out afresh: with no source newer than its own
        // last working out, `update` would keep what it had. Other traits written from outside
        // keep theirs. The kept sources name traits by index, and those after it move down one:
        // a second removal before the next update must find the right readers too.
        let k32 = k as u32;
        for t in self.tiles.iter_mut().filter(|t| t.live) {
            for tr in &mut t.traits {
                let mut read = false;
                for source in tr.sources.iter_mut().filter(|s| s.0 == tile) {
                    read |= source.1 == k32;
                    if source.1 > k32 {
                        source.1 -= 1;
                    }
                }
                if read {
                    tr.evaluated = 0;
                }
                tr.sources.retain(|&(s, j)| !(read && s == tile && j == k32));
            }
        }
        let slot = self.slot(tile).expect("a loaded tile");
        self.tiles[slot].traits.remove(k);
        self.tiles[slot].trait_hashes.remove(k);
        self.tiles[slot].expr_traits.retain_mut(|x| {
            if *x == k32 {
                return false;
            }
            if *x > k32 {
                *x -= 1;
            }
            true
        });
        self.tiles[slot].traits_shape += 1;
        if name.eq_ignore_ascii_case("childcount") {
            self.tiles[slot].explicit_childcount = false;
        }
        self.bump_shape();
        self.gen += 1;
        self.dirty = true;
        true
    }

    /// Whether a path names a live menu, tile or trait (`IsComponentLoaded`).
    pub fn is_loaded(&self, path: &str) -> bool {
        self.find_tile(path).is_some() || self.find_trait(path).is_some()
    }

    // ---- injection --------------------------------------------------------------------

    /// Puts XML text into the tile a path names (`InjectUIComponent`): tiles, templates,
    /// traits and includes, as if the tile's file had them at its end. False when the
    /// path names no tile.
    #[track_caller]
    pub fn inject_text(&mut self, src: &dyn Source, at: &str, text: &str) -> bool {
        self.entered("inject_text");
        let Some(tile) = self.find_tile(at) else { return false };
        let doc = xml::parse(text);
        for w in &doc.warnings {
            self.warnings.push(format!("injected into {at}: {w}"));
        }
        self.fill(src, tile, &doc.nodes, 0, &Origin::Injected);
        true
    }

    /// Puts a file's XML into the tile a path names (`InjectUIXML`); the file's path is
    /// under the game's folder (`data\menus\…`) [documented: InjectUIXML notes] or under
    /// `Data`. False when the path names no tile or the file is not there.
    #[track_caller]
    pub fn inject_file(&mut self, src: &dyn Source, at: &str, file: &str) -> bool {
        self.entered("inject_file");
        let f = file.replace('/', "\\");
        let f = match f.get(..5) {
            Some(head) if head.eq_ignore_ascii_case("data\\") => &f[5..],
            _ => &f[..],
        };
        let Some(bytes) = src.read(f) else { return false };
        self.inject_text(src, at, &xml::decode(&bytes))
    }

    /// Makes a tile from a template under the tile a path names
    /// (`AddTileFromTemplate`): the template is looked for on that tile and then on the
    /// tiles above it [hypothesis], the new tile named `name` when given. Returns it.
    #[track_caller]
    pub fn add_from_template(&mut self, src: &dyn Source, at: &str, template: &str, name: Option<&str>) -> Option<TileId> {
        self.entered("add_from_template");
        self.add_template(src, at, template, name, Origin::Template)
    }

    /// [design] Make engine-owned template copies, including every top-level tile and
    /// descendant in the template body. These copies are excluded from mod tile roots.
    #[track_caller]
    pub fn add_code_tile_from_template(&mut self, src: &dyn Source, at: &str, template: &str, name: Option<&str>) -> Option<TileId> {
        self.entered("add_code_tile_from_template");
        self.add_template(src, at, template, name, Origin::Code)
    }

    /// [design] Like [`Ui::add_code_tile_from_template`], attaching directly under `parent`
    /// by [`TileId`] instead of resolving a string path. Avoids sibling collisions when
    /// multiple list rows share the same display name or contain path delimiters (`/`, `\`,
    /// `:`).
    #[track_caller]
    pub fn add_code_tile_from_template_under(&mut self, src: &dyn Source, parent: TileId, template: &str, name: Option<&str>) -> Option<TileId> {
        self.entered("add_code_tile_from_template_under");
        self.add_template_under(src, parent, template, name, Origin::Code)
    }

    fn add_template(&mut self, src: &dyn Source, at: &str, template: &str, name: Option<&str>, origin: Origin) -> Option<TileId> {
        let parent = self.find_tile(at)?;
        self.add_template_under(src, parent, template, name, origin)
    }

    fn add_template_under(&mut self, src: &dyn Source, parent: TileId, template: &str, name: Option<&str>, origin: Origin) -> Option<TileId> {
        if !self.is_live(parent) {
            return None;
        }
        let mut look = Some(parent);
        let mut body = loop {
            let t = look?;
            if let Some(tp) = self.tile(t).templates.iter().find(|tp| tp.name.eq_ignore_ascii_case(template)) {
                break tp.body.clone();
            }
            look = self.tile(t).parent;
        };
        if let Some(n) = name {
            if let Some(Node::Element(first)) = body.iter_mut().find(|node| matches!(node, Node::Element(e) if is_tile(e))) {
                if let Some((_, val)) = first.attrs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("name")) {
                    *val = n.to_string();
                } else {
                    first.attrs.push(("name".into(), n.to_string()));
                }
            }
        }
        let before = self.tile(parent).children.len();
        self.fill(src, parent, &body, 0, &origin);
        let made = *self.tile(parent).children.get(before)?;
        Some(made)
    }

    /// [design] Top-most live mod-added roots in file order, including the menu itself
    /// when loaded loose. Loose provenance is captured at load; code tiles are not roots.
    pub fn mod_added_tiles(&self, menu: TileId) -> Vec<TileId> {
        if !self.tile(menu).live || self.tile(menu).kind != "menu" {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut stack = vec![menu];
        while let Some(id) = stack.pop() {
            let tile = self.tile(id);
            if !tile.live {
                continue;
            }
            if tile.origin.mod_added() || self.uio_tiles.contains(&id) {
                out.push(id);
            } else {
                stack.extend(tile.children.iter().rev().copied());
            }
        }
        out
    }

    /// [design] Whether [`Ui::mod_added_tiles`] of `menu` has any, without walking it:
    /// from the count kept as tiles are made, unloaded or change origin, so the engine may
    /// ask every frame [design: #938].
    pub fn has_mod_added_tiles(&self, menu: TileId) -> bool {
        let tile = self.tile(menu);
        tile.live && tile.kind == "menu" && self.mod_tiles.get(&menu).is_some_and(|&n| n > 0)
    }

    /// [design] The name paths ([`Ui::path_of`], lower case) of every live tile under
    /// `root`, itself included: what a menu file's tiles are called when one source builds
    /// them, to tell a loose replacement's own tiles from the archived file's
    /// (menu-xml.md §7).
    pub fn name_paths(&self, root: TileId) -> HashSet<String> {
        let mut out = HashSet::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let tile = self.tile(id);
            if tile.live {
                out.insert(self.path_of(id).to_ascii_lowercase());
                stack.extend(tile.children.iter().copied());
            }
        }
        out
    }

    /// [design] [`Ui::name_paths`] of the menu file at `path` as `src` builds it alone, in
    /// a tree of its own (`None` when the file cannot be read): the vanilla names a loose
    /// replacement is compared against.
    pub fn file_name_paths(src: &dyn Source, path: &str) -> Option<HashSet<String>> {
        let mut ui = Ui::new();
        let root = ui.load_menu(src, path).ok()?;
        Some(ui.name_paths(root))
    }

    /// [design] [`Ui::file_name_paths`] of the archived copy of the menu file at `path`
    /// ([`Archived`]), also under a loose replacement: the names [`Ui::mod_tile_drawn`]
    /// compares a loose file's tiles with (#975). `None` when no archive has the file.
    pub fn archived_name_paths(src: &dyn Source, path: &str) -> Option<HashSet<String>> {
        Ui::file_name_paths(&Archived(src), path)
    }

    /// [design] Whether a tile inside a root of [`Ui::mod_added_tiles`] is the mod's own,
    /// to draw over NVForever's HUD: never a `Code` tile; always an `Injected` or
    /// `Template` one; a `File` tile (a loose replacement's, or an archived prefab's it
    /// includes) only when `vanilla` holds the archived file's names
    /// ([`Ui::file_name_paths`]) and not this tile's name path. Without the vanilla names no
    /// `File` tile is drawn, except a loose UIO include and its file descendants [design].
    pub fn mod_tile_drawn(&self, id: TileId, vanilla: Option<&HashSet<String>>) -> bool {
        match &self.tile(id).origin {
            Origin::Code => false,
            Origin::Injected | Origin::Template => true,
            Origin::File { .. } => {
                // [design] A loose UIO file is an explicit addition, even when no
                // archived comparison exists or a vanilla tile has the same name.
                self.uio_tiles.contains(&id) || vanilla.is_some_and(|names| !names.contains(&self.path_of(id).to_ascii_lowercase()))
            }
        }
    }

    // ---- working traits out -----------------------------------------------------------

    /// Works out every trait with an expression whose sources changed since it was last
    /// worked out (all of them the first time), again and again until nothing changes.
    /// Returns how many trait values changed. Nothing to do when nothing changed since it
    /// last finished.
    pub fn update(&mut self) -> usize {
        if self.shape != self.shape_at_update {
            self.shape_at_update = self.shape;
            self.reshaped_run += 1;
        } else {
            self.reshaped_run = 0;
        }
        if !self.dirty {
            return 0;
        }
        let mut changed = 0;
        let mut sources = Vec::new();
        let mut struct_deps = Vec::new();
        #[cfg(test)]
        {
            self.passes = 0;
        }
        for _ in 0..MAX_PASSES {
            #[cfg(test)]
            {
                self.passes += 1;
            }
            let mut this_pass = 0;
            let roots_shape = self.roots_shape;
            for id in 0..self.tiles.len() {
                if !self.tiles[id].live || self.tiles[id].expr_traits.is_empty() {
                    continue;
                }
                let tid = self.id_at(id);
                let root = self.tiles[id].root;
                let root_slot = self.slot(root);
                let root_shape = root_slot.map_or(0, |r| self.tiles[r].root_shape);
                let expr_len = self.tiles[id].expr_traits.len();
                for j in 0..expr_len {
                    let k = self.tiles[id].expr_traits[j] as usize;
                    let tr = &self.tiles[id].traits[k];
                    let root_unchanged = tr.seen_root_shape == root_shape;
                    let roots_unchanged = tr.seen_roots_shape == roots_shape;
                    // What it read last (trait sources and structural resolution deps),
                    // unchanged since: working it out would find the same and skip it.
                    let skip = tr.evaluated != 0
                        && tr.seen_valid
                        && tr.struct_deps.iter().all(|&dep| self.struct_dep_holds(dep, tr.evaluated, root_unchanged, roots_unchanged))
                        && tr.sources.iter().all(|&(t, j)| {
                            self.slot(t).is_some_and(|i| self.tiles[i].traits.get(j as usize).is_some_and(|st| st.changed <= tr.evaluated))
                        });
                    #[cfg(test)]
                    let skip = skip && !self.never_skip;
                    if skip {
                        if !root_unchanged || !roots_unchanged {
                            let tr = &mut self.tiles[id].traits[k];
                            tr.seen_root_shape = root_shape;
                            tr.seen_roots_shape = roots_shape;
                        }
                        continue;
                    }
                    #[cfg(test)]
                    {
                        self.evaluations += 1;
                    }
                    let mut deps = 0;
                    sources.clear();
                    struct_deps.clear();
                    let v = {
                        let ops = self.tiles[id].traits[k].expr.as_deref().expect("checked expr.is_some()");
                        self.eval_reading(tid, ops, &mut deps, &mut sources, Some(&mut struct_deps))
                    };
                    if let Some(r) = root_slot {
                        self.tiles[r].root_evaluated = true;
                    }
                    let tr = &mut self.tiles[id].traits[k];
                    tr.sources.clone_from(&sources);
                    tr.struct_deps.clone_from(&struct_deps);
                    tr.seen_valid = true;
                    tr.seen_root_shape = root_shape;
                    tr.seen_roots_shape = roots_shape;
                    if tr.evaluated != 0 && deps <= tr.evaluated {
                        continue;
                    }
                    let differs = v.differs(&tr.value);
                    self.gen += 1;
                    let gen = self.gen;
                    let tr = &mut self.tiles[id].traits[k];
                    tr.evaluated = gen;
                    if differs {
                        tr.value = v;
                        tr.changed = gen;
                        this_pass += 1;
                    }
                }
            }
            changed += this_pass;
            if this_pass == 0 {
                self.dirty = false;
                return changed;
            }
        }
        self.warnings.push(format!("update: traits still changing after {MAX_PASSES} passes (a loop?)"));
        changed
    }

    fn struct_dep_holds(&self, dep: StructDep, evaluated: u64, root_unchanged: bool, roots_unchanged: bool) -> bool {
        match dep {
            StructDep::Children { tile, seen } => self.slot(tile).is_some_and(|i| self.tiles[i].children_changed == seen),
            StructDep::ChildCount { tile } => {
                self.slot(tile).is_some_and(|i| !self.tiles[i].explicit_childcount && self.tiles[i].children_changed <= evaluated)
            }
            StructDep::MissingTrait { tile, seen_traits } => self.slot(tile).is_some_and(|i| self.tiles[i].traits_shape == seen_traits),
            StructDep::RootName { key, seen } => roots_unchanged || self.root_name_version(key) == seen,
            StructDep::DescendantName { root, key, seen } => root_unchanged || (self.is_live(root) && self.descendant_name_version(root, key) == seen),
        }
    }

    fn record_dep(deps: &mut Option<&mut Vec<StructDep>>, dep: StructDep) {
        if let Some(list) = deps.as_deref_mut() {
            if !list.contains(&dep) {
                list.push(dep);
            }
        }
    }

    /// Whether [`Ui::update`] has something to work out.
    #[cfg(test)]
    pub(crate) fn dirty(&self) -> bool {
        self.dirty
    }

    /// The tile a source names, seen from `me`.
    pub fn resolve(&self, me: TileId, src: &Src) -> Option<TileId> {
        self.resolve_recording(me, src, None)
    }

    fn resolve_recording(&self, me: TileId, src: &Src, mut struct_deps: Option<&mut Vec<StructDep>>) -> Option<TileId> {
        let parent = |t: TileId| self.tile(t).parent;
        let child_named = |t: TileId, n: &str| self.tile(t).children.iter().copied().find(|&c| self.tile(c).name.eq_ignore_ascii_case(n));
        match src {
            Src::Me => Some(me),
            Src::Parent => parent(me),
            Src::Grandparent => parent(parent(me)?),
            Src::Ancestor(n) => {
                // Tiles are never reparented or renamed after creation, so ancestor
                // lookup for a live tile never changes [design].
                let mut at = parent(me);
                while let Some(t) = at {
                    if self.tile(t).name.eq_ignore_ascii_case(n) {
                        return Some(t);
                    }
                    at = parent(t);
                }
                None
            }
            Src::Child(None) => {
                Self::record_dep(&mut struct_deps, StructDep::Children { tile: me, seen: self.tile(me).children_changed });
                self.tile(me).children.first().copied()
            }
            Src::Child(Some(n)) => {
                Self::record_dep(&mut struct_deps, StructDep::Children { tile: me, seen: self.tile(me).children_changed });
                child_named(me, n)
            }
            // A menu's siblings are the other menus [observed: Vanilla UI Plus's start menu
            // reads `<copy src="sibling(HUDMainMenu)" trait="_VUI+"/>` on its root, and its
            // own check shows an "installation error" unless that gives its 1, #2298].
            Src::Sibling(Some(n)) if parent(me).is_none() => {
                let key = name_key(n);
                Self::record_dep(&mut struct_deps, StructDep::RootName { key, seen: self.root_name_version(key) });
                self.root_named(n).filter(|&r| r != me)
            }
            Src::Sibling(n) => {
                let p = parent(me)?;
                Self::record_dep(&mut struct_deps, StructDep::Children { tile: p, seen: self.tile(p).children_changed });
                let kids = &self.tile(p).children;
                match n {
                    Some(n) => kids.iter().copied().find(|&c| c != me && self.tile(c).name.eq_ignore_ascii_case(n)),
                    None => {
                        let at = kids.iter().position(|&c| c == me)?;
                        at.checked_sub(1).map(|k| kids[k])
                    }
                }
            }
            Src::Screen => self.screen,
            Src::Globals => self.globals,
            Src::Strings => self.strings,
            Src::Io => Some(self.root_of(me)),
            Src::Named(n) => {
                let root = self.root_of(me);
                let key = name_key(n);
                // `src="me"`, the brackets left off, is the tile itself when no tile bears
                // the name [observed: MCM 1.5's Options.xml, `<_x1> <copy src="me"
                // trait="x"/> <add> 20 </add> </_x1>` on MCM_Options, whose rows stand in
                // the right pane in the original; read as a name it gave 0 and the rows
                // stood at the screen's left, the owner's run of 2026-11-12].
                // Likewise every source that takes no argument: `parent`, `grandparent`,
                // `child`, `sibling`, `globals`, `screen`, `io` and `strings` [observed:
                // Vanilla UI Plus writes them so throughout its menus, `parent` 1,739 times,
                // `sibling` 614, `child` 15 in the Blue Moon list's files; its MCM
                // OptionRect.xml places each option's title by `<copy src="parent"
                // trait="_y"/>`, and read as a name every title stood at y 0, one over the
                // other at the top of the screen, the owner's run of 2026-11-13, #2298].
                Self::record_dep(&mut struct_deps, StructDep::DescendantName { root, key, seen: self.descendant_name_version(root, key) });
                if let Some(found) = self.find_named(root, n) {
                    return Some(found);
                }
                Self::record_dep(&mut struct_deps, StructDep::RootName { key, seen: self.root_name_version(key) });
                if let Some(found) = self.root_named(n) {
                    return Some(found);
                }
                let bare = Src::parse(&format!("{n}()"));
                match bare {
                    Src::Me | Src::Parent | Src::Grandparent | Src::Child(None) | Src::Sibling(None) | Src::Globals | Src::Screen | Src::Io | Src::Strings => {
                        self.resolve_recording(me, &bare, struct_deps)
                    }
                    _ => None,
                }
            }
        }
    }

    /// The first tile of that name under `root`, in file order.
    fn find_named(&self, root: TileId, name: &str) -> Option<TileId> {
        let names = self.root_names.get(&root)?.get_or_init(|| {
            let mut names = HashMap::new();
            let mut stack = vec![root];
            while let Some(t) = stack.pop() {
                #[cfg(test)]
                self.named_source_visits.fetch_add(1, Ordering::Relaxed);
                if t != root {
                    names.entry(self.tile(t).name.to_ascii_lowercase()).or_insert(t);
                }
                stack.extend(self.tile(t).children.iter().rev().copied());
            }
            names
        });
        // Avoid heap-allocating a lowercased String on the hot path for ASCII names <= 128 bytes.
        let bytes = name.as_bytes();
        if bytes.len() <= 128 && bytes.is_ascii() {
            let mut buf = [0u8; 128];
            for (dst, &b) in buf.iter_mut().zip(bytes) {
                *dst = b.to_ascii_lowercase();
            }
            // SAFETY: ASCII lowercase bytes are valid UTF-8.
            let lower = unsafe { std::str::from_utf8_unchecked(&buf[..bytes.len()]) };
            names.get(lower).copied()
        } else {
            names.get(&name.to_ascii_lowercase()).copied()
        }
    }

    fn read_trait(
        &self,
        me: TileId,
        src: &Src,
        name: &str,
        deps: &mut u64,
        sources: &mut Vec<(TileId, u32)>,
        mut struct_deps: Option<&mut Vec<StructDep>>,
    ) -> Value {
        let Some(t) = self.resolve_recording(me, src, struct_deps.as_deref_mut()) else { return Value::default() };
        match self.tile(t).trait_index(name) {
            Some(k) => {
                let tr = &self.tile(t).traits[k];
                *deps = (*deps).max(tr.changed);
                sources.push((t, k as u32));
                tr.value.clone()
            }
            // [design: menu-xml.md §6.1] Explicit list counts keep precedence.
            // Do not insert a trait: absence also guards pause-column initialization.
            None if name.eq_ignore_ascii_case("childcount") => {
                let tile = self.tile(t);
                Self::record_dep(&mut struct_deps, StructDep::ChildCount { tile: t });
                *deps = (*deps).max(tile.children_changed);
                Value::Num(tile.children.len() as f64)
            }
            // A trait not there reads 0 [hypothesis].
            None => {
                let tile = self.tile(t);
                Self::record_dep(&mut struct_deps, StructDep::MissingTrait { tile: t, seen_traits: tile.traits_shape });
                Value::default()
            }
        }
    }

    /// Works out an operator list on tile `me`; `deps` takes the latest change count of
    /// anything read.
    pub fn eval(&self, me: TileId, ops: &[Op], deps: &mut u64) -> Value {
        self.eval_reading(me, ops, deps, &mut Vec::new(), None)
    }

    /// [`Ui::eval`], and the traits read (tile, index) put in `sources` (plus structural
    /// resolution conditions in `struct_deps`). What it reads depends only on those
    /// conditions and the values read (`_filename_<n>` on a value worked out before), so
    /// while none changed it reads the same.
    fn eval_reading(
        &self,
        me: TileId,
        ops: &[Op],
        deps: &mut u64,
        sources: &mut Vec<(TileId, u32)>,
        mut struct_deps: Option<&mut Vec<StructDep>>,
    ) -> Value {
        let mut acc = Value::default();
        for (k, op) in ops.iter().enumerate() {
            let arg = match &op.arg {
                Arg::None => Value::default(),
                Arg::Const(v) => v.clone(),
                Arg::Seq(inner) => self.eval_reading(me, inner, deps, sources, struct_deps.as_deref_mut()),
                Arg::Ref { src, name } => {
                    // `<copy … trait="_filename_"/>` after a number reads `_filename_<n>`
                    // [hypothesis: the meter and barter files pick a picture so]; after a
                    // constant with a known number, by it (`&false;` 0, `&true;` 1,
                    // menu-xml.md §2.3; an unknown constant names no suffix) [hypothesis:
                    // the meter prefab's `_SolidMeter` and vertical_fade_line.xml's
                    // `_bottom_to_top` are `&false;`/`&true;` before the copy, and their
                    // `_filename_0`/`_Filename_1` are the two pictures; T15a].
                    let name: std::borrow::Cow<'_, str> = match (&acc, name.ends_with('_')) {
                        (Value::Num(n), true) if op.kind == OpKind::Copy => std::borrow::Cow::Owned(format!("{name}{}", *n as i64)),
                        (v, true) if op.kind == OpKind::Copy => match v.sym_name().as_deref().and_then(crate::value::sym_number) {
                            Some(n) => std::borrow::Cow::Owned(format!("{name}{}", n as i64)),
                            None => std::borrow::Cow::Borrowed(name.as_str()),
                        },
                        _ => std::borrow::Cow::Borrowed(name.as_str()),
                    };
                    self.read_trait(me, src, &name, deps, sources, struct_deps.as_deref_mut())
                }
            };
            acc = apply(op.kind, acc, arg, me, k);
        }
        acc
    }
}

fn is_tile(e: &Element) -> bool {
    TILE_KINDS.iter().any(|k| e.is(k))
}

fn uio_include(file: &str) -> Node {
    Node::Element(Element {
        name: "include".into(),
        attrs: vec![("src".into(), file.into()), ("nvf_uio".into(), "true".into())],
        ..Element::default()
    })
}

fn named_nodes<'a>(nodes: &'a mut [Node], name: &str) -> Option<&'a mut Vec<Node>> {
    for node in nodes {
        if let Node::Element(e) = node {
            if is_tile(e) && e.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                return Some(&mut e.children);
            }
            if let Some(found) = named_nodes(&mut e.children, name) {
                return Some(found);
            }
        }
    }
    None
}

fn untargeted_nodes(nodes: &mut Vec<Node>) -> Option<&mut Vec<Node>> {
    match nodes.iter().position(|n| matches!(n, Node::Element(e) if e.is("menu"))) {
        Some(i) => match &mut nodes[i] {
            Node::Element(e) => Some(&mut e.children),
            _ => unreachable!(),
        },
        None => Some(nodes),
    }
}

/// An include's file: under `menus\prefabs\` [observed: every vanilla include names a
/// file there; MCM's `MCM\MCM.xml` a file in a folder under it], collapsing `.` and
/// `..` path segments so sibling paths under `menus\` (such as `..\sibling\file.xml`)
/// match UIO's path resolution while preserving authored case [design].
fn prefab_path(file: &str) -> String {
    let raw = format!("menus\\prefabs\\{}", file.trim().replace('/', "\\"));
    let mut parts = Vec::new();
    for part in raw.split('\\') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("\\")
}

fn parse_seg(seg: &str) -> (&str, Option<usize>) {
    match seg.rsplit_once(':') {
        Some((n, k)) => match k.trim().parse::<usize>() {
            Ok(idx) => (n, Some(idx)),
            Err(_) => (seg, None),
        },
        None => (seg, None),
    }
}

fn split_slice<'a, 'b>(path: &'a str, stack: &'b mut [&'a str; 16], heap: &'b mut Vec<&'a str>) -> &'b [&'a str] {
    let mut len = 0usize;
    for s in path.split(['/', '\\']).map(str::trim).filter(|s| !s.is_empty()) {
        if heap.is_empty() && len < stack.len() {
            stack[len] = s;
            len += 1;
        } else {
            if heap.is_empty() {
                heap.extend_from_slice(&stack[..len]);
            }
            heap.push(s);
        }
    }
    if heap.is_empty() { &stack[..len] } else { heap.as_slice() }
}

/// Invalid/overflowing extra-operator results do not enter renderer traits [hypothesis].
fn finite_extra(value: f64) -> Value {
    Value::Num(if value.is_finite() { value } else { 0.0 })
}

/// Signed 32-bit integer bitwise/shift operators (`mcm-rgb-integer-consumers.md`).
/// [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + UI Organizer 230, 2026-11-24,
/// `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room
/// staging copy (synthetic prefab XML staged strictly inside the isolated copy and removed
/// after exit; read-only `PrintToConsole` probe; zero writes to game install or Saves),
/// `w4e_10_mcm_rgb_negative_frac_shift` (`tools/capture/nvse/catalogs/wave4e_mods.json`,
/// PR #3803 / #3802):
/// `PROBE|w4e_10_mcm_rgb_negative_frac_shift|rgb=255,128,64|land(neg=255,frac=4)|shl(neg=-20,frac=8,s32=0)|shr(neg=-4)`;
/// zero DLL, PDB, or third-party source consulted]
/// Finite operands are rounded to nearest `i32` (`.5` half-away-from-zero tie-breaking
/// `[design]`). Non-numeric, nonfinite, out-of-`i32`-range operands and negative shift counts
/// leave `acc` unchanged `[design]`. Shift counts `>= 32` yield `0` (`shl` `[observed]`;
/// `shr` `[design]`).
fn integer_extra(kind: OpKind, acc: Value, arg: Value) -> Value {
    let to_i32 = |value: &Value| match value {
        Value::Num(n) if n.is_finite() => {
            let rounded = n.round();
            if (i32::MIN as f64..=i32::MAX as f64).contains(&rounded) {
                Some(rounded as i32)
            } else {
                None
            }
        }
        _ => None,
    };
    let (Some(x), Some(y)) = (to_i32(&acc), to_i32(&arg)) else { return acc };
    let result = match kind {
        OpKind::BitAnd => Some(x & y),
        OpKind::ShiftLeft if y < 0 => None,
        OpKind::ShiftLeft if y >= 32 => Some(0),
        OpKind::ShiftLeft => Some(x.wrapping_shl(y as u32)),
        OpKind::ShiftRight if y < 0 => None,
        OpKind::ShiftRight if y >= 32 => Some(0),
        OpKind::ShiftRight => Some(x >> (y as u32)),
        _ => None,
    };
    result.map(|value| Value::Num(f64::from(value))).unwrap_or(acc)
}

fn apply(kind: OpKind, acc: Value, a: Value, me: TileId, k: usize) -> Value {
    let (x, y) = (acc.num(), a.num());
    let num = Value::Num;
    match kind {
        OpKind::Copy | OpKind::Ref => a,
        OpKind::Add => num(x + y),
        OpKind::Sub => num(x - y),
        OpKind::Mul => num(x * y),
        // Dividing by 0 leaves the value [hypothesis].
        OpKind::Div => num(if y == 0.0 { x } else { x / y }),
        OpKind::Mod => num(if y == 0.0 { x } else { x % y }),
        OpKind::Min => num(x.min(y)),
        OpKind::Max => num(x.max(y)),
        // The rounding operators work on the value; what is written in them (always 0
        // or 1 in the vanilla files [observed]) is not used [hypothesis].
        OpKind::Floor => num(x.floor()),
        OpKind::Ceil => num(x.ceil()),
        OpKind::Round => num(x.round()),
        OpKind::Trunc => num(x.trunc()),
        OpKind::Abs => num(x.abs()),
        // [documented: ui-neg-operator.md] Unlike abs/floor, neg uses the argument.
        OpKind::Neg => num(-y),
        // UIO's unary arguments and accumulated exponentiation. Only the added
        // operators use this finite-output policy; existing arithmetic is unchanged.
        OpKind::Sin => finite_extra(y.to_radians().sin()),
        OpKind::Cos => finite_extra(y.to_radians().cos()),
        OpKind::Log => finite_extra(y.ln()),
        OpKind::Pow => finite_extra(x.powf(y)),
        // [documented: ui-bit-test-operator.md] The argument selects one bit.
        // Limit conversion to consecutive exactly representable nonnegative
        // integers. Native signed/coercion/width behavior remains unobserved.
        OpKind::BitTest => {
            let valid = x.is_finite() && (0.0..=9_007_199_254_740_992.0).contains(&x)
                && x.fract() == 0.0 && y.is_finite() && y >= 0.0 && y.fract() == 0.0;
            Value::from_bool(valid && y <= 53.0 && (x as u64 & (1_u64 << y as u32)) != 0)
        }
        OpKind::BitAnd | OpKind::ShiftLeft | OpKind::ShiftRight => integer_extra(kind, acc, a),
        // A whole number from 0 below the argument, the same for this tile and operator
        // every time [design: when the original draws is not known].
        OpKind::Rand => {
            let n = y.max(1.0) as u64;
            let h = (me.index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (k as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
            num((h >> 33) as f64 % n as f64)
        }
        OpKind::Eq => Value::from_bool(acc.same(&a)),
        OpKind::Neq => Value::from_bool(!acc.same(&a)),
        OpKind::Lt => Value::from_bool(x < y),
        OpKind::Lte => Value::from_bool(x <= y),
        OpKind::Gt => Value::from_bool(x > y),
        OpKind::Gte => Value::from_bool(x >= y),
        OpKind::And => Value::from_bool(acc.truthy() && a.truthy()),
        OpKind::Or => Value::from_bool(acc.truthy() || a.truthy()),
        OpKind::Not => Value::from_bool(!a.truthy()),
        // A false condition makes what was worked out so far 0, and the operators after
        // it go on from there [hypothesis: fits every vanilla use, menu-xml.md §4.3].
        OpKind::OnlyIf => {
            if a.truthy() {
                acc
            } else {
                Value::default()
            }
        }
        OpKind::OnlyIfNot => {
            if a.truthy() {
                Value::default()
            } else {
                acc
            }
        }
    }
}

#[cfg(test)]
#[path = "trait_hash_and_expr_index_tests.rs"]
mod trait_hash_and_expr_index_tests;

#[cfg(test)]
mod write_serial_tests {
    use super::*;

    #[test]
    fn trait_write_serial_exhaustion_keeps_legacy_values_without_reusing_tokens() {
        let mut ui = Ui::new();
        let screen = ui.screen();
        ui.set_trait(screen, "x", Value::Num(12.0));
        let old = ui.trait_write_serial(screen, "x").unwrap();
        let clock = ui.last_write_serial;
        assert!(!ui.set_float("screen()/missing", 1.0));
        assert!(!ui.set_existing(screen, "missing", Value::Num(1.0)));
        assert_eq!(ui.last_write_serial, clock);
        ui.last_write_serial = u64::MAX - 1;
        ui.set_trait(screen, "x", Value::Num(12.0));
        let last = ui.trait_write_serial(screen, "x").unwrap();
        assert_ne!(old, last);
        assert_eq!(ui.last_write_serial, u64::MAX);
        ui.set_trait(screen, "x", Value::Num(13.0));
        assert_eq!(ui.trait_write_serial(screen, "x"), None);
        assert_eq!(ui.tile(screen).get("x"), Some(&Value::Num(13.0)));
        ui.set_trait(screen, "x", Value::Num(14.0));
        assert_eq!(ui.tile(screen).get("x"), Some(&Value::Num(14.0)));
        assert_eq!(ui.trait_write_serial(screen, "x"), None);
        let src = MemSource::default();
        assert!(ui.inject_text(&src, "screen()", "<x>15</x><y>24</y>"));
        assert_eq!(ui.tile(screen).get("x"), Some(&Value::Num(15.0)));
        assert_eq!(ui.tile(screen).get("y"), Some(&Value::Num(24.0)));
        assert_eq!(ui.trait_write_serial(screen, "x"), None);
        assert_eq!(ui.trait_write_serial(screen, "y"), None);
        assert_eq!(ui.last_write_serial, u64::MAX);
    }
    #[test]
    fn conditional_batch_rejects_an_equal_external_writer_without_partial_values() {
        let mut ui=Ui::new();let tile=ui.screen();
        ui.set_trait(tile,"x",Value::Num(1.0));ui.set_trait(tile,"y",Value::Num(2.0));
        let x=ui.trait_write_serial(tile,"x").unwrap();let y=ui.trait_write_serial(tile,"y").unwrap();
        ui.set_trait(tile,"x",Value::Num(1.0));let before=ui.trait_write_counts();
        assert!(ui.set_existing_batch_if_unchanged(&[
            ConditionalTraitWrite {tile,name:"y",expected:y,value:Value::Num(9.0)},
            ConditionalTraitWrite {tile,name:"x",expected:x,value:Value::Num(8.0)},
        ]).is_none());
        assert_eq!(ui.trait_write_counts(),before);assert_eq!(ui.tile(tile).get("y"),Some(&Value::Num(2.0)));
    }
    #[test]
    fn conditional_batch_checks_exhaustion_before_any_accepted_legacy_write() {
        let mut ui=Ui::new();let tile=ui.screen();ui.set_trait(tile,"x",Value::Num(1.0));ui.set_trait(tile,"y",Value::Num(2.0));
        let x=ui.trait_write_serial(tile,"x").unwrap();let y=ui.trait_write_serial(tile,"y").unwrap();
        ui.last_write_serial=u64::MAX-1;let before=ui.trait_write_counts();
        assert!(ui.set_existing_batch_if_unchanged(&[
            ConditionalTraitWrite {tile,name:"x",expected:x,value:Value::Num(8.0)},
            ConditionalTraitWrite {tile,name:"y",expected:y,value:Value::Num(9.0)},
        ]).is_none());assert_eq!(ui.last_write_serial,u64::MAX-1);assert_eq!(ui.trait_write_counts(),before);
        assert_eq!(ui.tile(tile).get("x"),Some(&Value::Num(1.0)));assert_eq!(ui.tile(tile).get("y"),Some(&Value::Num(2.0)));
    }
    #[test]
    fn conditional_batch_assigns_all_sinks_and_rejects_aliasing_duplicates() {
        let mut ui=Ui::new();let tile=ui.screen();ui.set_trait(tile,"x",Value::Num(1.0));ui.set_trait(tile,"y",Value::Num(2.0));
        let x=ui.trait_write_serial(tile,"x").unwrap();let y=ui.trait_write_serial(tile,"y").unwrap();
        assert!(ui.set_existing_batch_if_unchanged(&[ConditionalTraitWrite {tile,name:"x",expected:x,value:Value::Num(3.0)},ConditionalTraitWrite {tile,name:"X",expected:x,value:Value::Num(4.0)}]).is_none());
        let got=ui.set_existing_batch_if_unchanged(&[ConditionalTraitWrite {tile,name:"x",expected:x,value:Value::Num(3.0)},ConditionalTraitWrite {tile,name:"y",expected:y,value:Value::Num(4.0)}]).unwrap();
        assert_eq!(got.len(),2);assert_ne!(got[0],x);assert_ne!(got[0],got[1]);assert_eq!(ui.tile(tile).get("x"),Some(&Value::Num(3.0)));assert_eq!(ui.tile(tile).get("y"),Some(&Value::Num(4.0)));
    }

    #[test]
    fn conditional_batch_rejects_expression_sink_before_any_plain_sink_write() {
        let src = MemSource::default().with("batch.xml", r#"<menu name="Batch"><x>1</x><y><copy src="me()" trait="x"/></y></menu>"#);
        let mut ui = Ui::new();
        let tile = ui.load_menu(&src, "batch.xml").unwrap();
        ui.update();
        let x = ui.trait_write_serial(tile, "x").unwrap();
        let y = ui.trait_write_serial(tile, "y").unwrap();
        let before = ui.trait_write_counts();
        assert!(ui.tile(tile).traits[ui.tile(tile).trait_index("y").unwrap()].expr.is_some());
        assert!(ui.set_existing_batch_if_unchanged(&[
            ConditionalTraitWrite { tile, name: "x", expected: x, value: Value::Num(8.) },
            ConditionalTraitWrite { tile, name: "y", expected: y, value: Value::Num(9.) },
        ]).is_none());
        assert_eq!(ui.trait_write_counts(), before);
        assert_eq!(ui.tile(tile).get("x"), Some(&Value::Num(1.)));
        assert_eq!(ui.tile(tile).get("y"), Some(&Value::Num(1.)));
    }

    #[test]
    fn conditional_batch_rejects_old_tile_generation_after_reload_without_partial_write() {
        let src = MemSource::default().with("batch.xml", r#"<menu name="Batch"><x>1</x></menu>"#);
        let mut ui = Ui::new();
        let old = ui.load_menu(&src, "batch.xml").unwrap();
        let expected = ui.trait_write_serial(old, "x").unwrap();
        let current = ui.load_menu(&src, "batch.xml").unwrap();
        assert_ne!(old, current);
        let now = ui.trait_write_serial(current, "x").unwrap();
        let before = ui.trait_write_counts();
        assert!(ui.set_existing_batch_if_unchanged(&[
            ConditionalTraitWrite { tile: current, name: "x", expected: now, value: Value::Num(8.) },
            ConditionalTraitWrite { tile: old, name: "x", expected, value: Value::Num(9.) },
        ]).is_none());
        assert_eq!(ui.trait_write_counts(), before);
        assert_eq!(ui.tile(current).get("x"), Some(&Value::Num(1.)));
    }

    #[test]
    fn indexed_name_and_path_number_repeated_siblings_only() {
        let xml = r#"<menu name="BarterMenu">
            <rect name="BarterCategories">
                <rect name="Header"/>
                <rect name="BC_Category"/>
                <rect name="BC_Category"/>
                <rect name="BC_Category"/>
            </rect>
        </menu>"#;
        let src = MemSource::default().with("barter.xml", xml);
        let mut ui = Ui::new();
        let root = ui.load_menu(&src, "barter.xml").unwrap();
        let group = ui.tile(root).children[0];
        let children = ui.tile(group).children.clone();
        assert_eq!(ui.indexed_name_of(children[0]), "Header");
        assert_eq!(ui.indexed_path_of(children[0]), "BarterMenu/BarterCategories/Header");
        assert_eq!(ui.indexed_name_of(children[1]), "BC_Category:0");
        assert_eq!(ui.indexed_path_of(children[1]), "BarterMenu/BarterCategories/BC_Category:0");
        assert_eq!(ui.indexed_name_of(children[2]), "BC_Category:1");
        assert_eq!(ui.indexed_name_of(children[3]), "BC_Category:2");
    }
}
