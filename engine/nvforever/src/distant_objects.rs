//! Distant objects (#3014; docs/design/graphics-enhanced.md §3.21): with the settings
//! page's Distant objects row at 8 or 12 cells, static scenery the distant LOD blocks do
//! not hold is drawn beyond the loaded cells (and beyond the full-detail ring,
//! full_detail.rs), out to that many grid steps from the camera's cell: from the
//! reference's own `<model>_lod.nif` when the load order has one beside its full model,
//! else from its full model when it is large enough for its distance. Off by default and
//! in both presets. Everything here is NVForever's own design, not the original's
//! behaviour.
//!
//! - **The original's rule** [documented: GECK wiki "Static", "World LOD"]: a base flagged
//!   "Visible When Distant" (record header flag 0x8000) has its references baked into the
//!   generated object LOD blocks when a `<model>_lod.nif` exists in the same folder. The
//!   blocks are made for one load order, so plugins and LOD packs added later change
//!   nothing until the blocks are made again.
//! - **What is drawn** ([`choose`]): exterior STAT and SCOL references that pass the
//!   ring's scenery filter (not deleted, not an imposter, primitive or marker, not hidden).
//!   A reference with a model override (`Streamer::model_overrides`: a destroyed object's
//!   stage, a swap) is not drawn here at all: when its override comes, its shapes are
//!   despawned at once from every distant cell holding it (drawn, replaced or being
//!   spawned), and when the overrides of a cell's references change either way, the cell
//!   is judged again ([`override_changes`]). Conservative first rule, free of doubling while
//!   the blocks match the load order: a reference flagged visible when distant (its own
//!   flag or its base's, in any plugin's version: `fnv_world::placed::visible_when_distant`)
//!   is left to the block while its cell's segment is not empty or not known yet
//!   (`lod::BlockContents`). Every other reference is drawn from its LOD model when one
//!   exists ([`lod_model`]: `<model>_lod.nif`, else `<model>_lod_0.nif`), else from its
//!   full model when its scaled bound radius (OBND) is at least [`SIZE_RATIO`] of its
//!   distance from the centre of the loaded grid's middle cell, else not at all.
//! - **Triangle budget** ([`fit_budget`], [`Budget`]): the full models drawn may sum to at
//!   most [`FULL_TRIANGLE_BUDGET`] triangles; past it the size ratio is raised for the
//!   farthest cells first (by [`RATIO_RAISE`] up to [`RATIO_MAX`]), and a full model that
//!   would still pass the cap is left out ([`admits`]). Logged when it bites. The sum is
//!   never kept as a running count: it is read from the cells alive (their retained
//!   models, the reservations of cells being spawned, the predecessors still fading out),
//!   so a cell dropped half-spawned or retired gives its share back by leaving. One
//!   rebuilt cell at a time counts without its predecessor's share, so the total passes
//!   the cap by at most that one predecessor's share, from the replacement's first full
//!   model until the predecessor has faded out. A model refused for the budget is
//!   recorded apart from one too small ([`Choice::Budget`]), and its cell is judged again
//!   once room for it comes back.
//! - **Draw-only**, as the ring's cells: a distant cell's entity carries [`DistantCell`]
//!   and `nvf_play::DrawOnlyCell`, never `world::CellEntity` or `nvf_play::RenderCell`; its
//!   models keep `RefVisual` (the simulation's enable state, `activate::sync_visuals` on
//!   every `RefChanged`, and moved references reach them) and carry `nvf_play::DrawOnly`
//!   (`world::SpawnMode::Scenery`). No collider, light, emitter or script; no sun shadow
//!   (`NotShadowCaster` on every mesh). The draw-distance sliders leave them alone
//!   (`draw_distance::DrawClass::Distant`).
//! - **Streaming** ([`stream`]): the cells [`distant_cells`] names, from one step beyond
//!   max(loaded radius, ring radius) out to the reach, are listed on the worker threads
//!   (never on this thread: a cell's placements are read there) and the lists kept within
//!   [`KEEP_LISTS`] steps beyond the reach. Lists start only while the loaded cells' and
//!   the ring's read-ahead are idle; cells are spawned nearest first within
//!   [`SPAWN_BUDGET`] a frame, after the ring, only while the loaded grid and the ring are
//!   complete, and only with models already prepared on the workers (a cell waits for
//!   its models; the next [`PREFETCH_CELLS`] cells' models are asked for meanwhile). The
//!   budget's fit ([`fit_budget`] over every candidate) runs on a worker too, one at a
//!   time. What the main thread's budget bounds: starting lists (a cell's reference list
//!   is copied from the index), the scan for cells to judge again and the cells planned
//!   and spawned are each stopped at the deadline between items; a single item is never
//!   split, so a frame can pass the budget by one cell's plan (its entries judged) or one
//!   reference's spawn (all the shapes of its model). Taking finished lists in is bounded
//!   by [`MAX_LISTING`]; the override check is one pass over the overrides. A settled frame
//!   (nothing under way, every wanted cell at rest and judged for now, the inputs the
//!   same: centre, radii, the loaded cells' and the ring's counts and fades, the blocks'
//!   revision, the overrides) returns after comparing them, without a pass over the cells;
//!   the reported time covers both systems. A worker's fit is taken only while it is for
//!   the centre and reach judged now. Going indoors
//!   or turning the distant LOD off hides everything at once and despawns it over the next
//!   frames, in the bounded slices the loaded cells use (`world::despawn_dying`); a travel
//!   or another worldspace drops everything at once (behind the loading screen).
//! - **Fades** ([`distant_range`], [`fade_distant`]): a distant cell in a square drawn by
//!   a loaded or ring cell draws the complement of that cell's fade step (the same
//!   references in full) and is held at rest as the ring's cells are (`full_detail::hand_off`),
//!   so each pixel is drawn once (the cover is the loaded or ring cell's scenery level,
//!   [`scenery_level`], never the ring's tree level); alone it fades in and out by its own
//!   step, and always dithers out with distance between `reach - 1` and `reach` cells from
//!   the camera. One range cannot multiply two dithers, so a fading mesh is drawn at the
//!   smaller of its cell's step and the reach's fade at its own distance ([`band_range`]),
//!   and nothing beyond the reach; at rest the plain distance fade takes over at the same
//!   level. Such a range depends on the eye's distance: it is given again when the box of
//!   the cell's meshes' positions changes zone against the band ([`cell_zone`]), and,
//!   across it, once the eye has moved [`EYE_STEP`]; a still eye walks no mesh.
//!   After Off the cells fade out with the last reach's fade. A rebuilt cell crossfades with the one it
//!   replaces only where that whole cell lies within `reach - 2` cells of the middle cell's
//!   centre ([`crossfade_ok`]); nearer the edge it takes over at once, since the old cell's
//!   complement could not also carry the reach's fade, and a predecessor that no longer
//!   passes (the reach shrank, the centre moved) goes at once.
//!   Cells leave by fading out past `reach + 1` (or out of a smaller new reach), or once
//!   the loaded or ring cell over them is fully drawn.
//! - **Re-judging** ([`Stamp`]): each cell keeps the centre and the ratio table it was
//!   judged for. When the loaded grid's centre has moved [`REJUDGE_STEPS`] cells, or the
//!   budget's fit changes the ratios, or the block of a cell holding flagged references
//!   answers differently than when it was judged, the cell is judged again as soon as it
//!   is at rest, alone and not being replaced (a cell fading, covered or replacing then
//!   catches up later; the scan goes on in a stable order from where it stopped); one
//!   whose drawn set changes is built again and crossfaded with the
//!   old one (the new one's step against the old one's complement). A full model already
//!   drawn stays while its radius is at least [`HYSTERESIS`] of the threshold, so nothing
//!   flickers at it.
//! - **Logs** (only while on): `distant objects: reach N cells (ratio R)` when it comes on;
//!   at the exit `distant objects summary: <cells> cells, <drawn> drawn (<lod> lod, <full>
//!   full), <held> held by blocks, <unknown> block unknown, <small> too small, <b> past
//!   the triangle budget, <t> ms on the main thread`. When the row goes Off, once the last
//!   cell has gone: `distant objects: off, 0 cells left`.

use crate::emittance::EmittanceState;
use crate::fade::{self, Fading};
use crate::full_detail::{self, covered, fades_out, hand_off, steps, FullDetailRing, Grid};
use crate::graphics::Graphics;
use crate::lod::{BlockContents, LodState};
use crate::world::{self, SpawnMode, StreamStats, Streamer, WorldData, WorldRoot};
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, Task};
use fnv_esm::{LoadOrder, Schema as EsmSchema};
use fnv_world::alt_textures::AltTexture;
use fnv_world::placed::{base_object, placement_visible_when_distant, BaseObject, Placement};
use fnv_world::CELL_SIZE;
use nvf_assets::model::ModelCtx;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The settings page's Distant objects row (`Graphics::distant_objects`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DistantObjects {
    /// Past the loaded cells only the LOD blocks (as the original).
    #[default]
    Off,
    /// Out to [`REACH_NEAR`] grid steps.
    Eight,
    /// Out to [`REACH_FAR`] grid steps.
    Twelve,
}

/// The nearer reach (grid steps from the camera's cell) [design].
pub const REACH_NEAR: i32 = 8;
/// The farther reach [design].
pub const REACH_FAR: i32 = 12;

impl DistantObjects {
    /// The row's choices in its order, by the names it shows and saves.
    pub const CHOICES: [(&'static str, DistantObjects); 3] = [("Off", DistantObjects::Off), ("8 cells", DistantObjects::Eight), ("12 cells", DistantObjects::Twelve)];

    /// Grid steps from the camera's cell the distant objects reach (0: off).
    pub fn reach(self) -> i32 {
        match self {
            DistantObjects::Off => 0,
            DistantObjects::Eight => REACH_NEAR,
            DistantObjects::Twelve => REACH_FAR,
        }
    }

    /// The row's name of the choice.
    pub fn name(self) -> &'static str {
        Self::CHOICES.iter().find(|(_, d)| *d == self).map_or("Off", |(n, _)| n)
    }

    /// For `Graphics::describe` (empty while off).
    pub fn describe(self) -> &'static str {
        match self {
            DistantObjects::Off => "",
            DistantObjects::Eight => "distant objects 8 cells",
            DistantObjects::Twelve => "distant objects 12 cells",
        }
    }
}

/// A reference without a LOD model is drawn from its full model when its scaled bound
/// radius is at least this part of its distance: 328 units at 8 cells, 492 at 12
/// [engineering choice; the angular rule of `shadow_thin.rs`].
pub const SIZE_RATIO: f32 = 0.01;

/// A full model already drawn stays while its radius is at least this part of the
/// threshold ([`size_passes`]): the hysteresis that keeps a model on the edge of the rule
/// from coming and going as the centre moves [design].
pub const HYSTERESIS: f32 = 0.8;

/// The size rule is judged again when the loaded grid's centre has moved this many grid
/// steps since the last judgement [design].
pub const REJUDGE_STEPS: i32 = 2;

/// The most triangles the full models drawn may sum to ([`fit_budget`]) [engineering
/// choice: the study counted 1,900-2,700 qualifying references within 12 cells at
/// Goodsprings, Primm and Novac, against a median full model of 4,968 triangles].
pub const FULL_TRIANGLE_BUDGET: u64 = 4_000_000;

/// The triangles counted for a full model not prepared yet (the median full model with a
/// LOD sibling in the base game, measured from the data) [engineering choice].
pub const UNKNOWN_TRIANGLES: u64 = 5_000;

/// Each raise of a cell ring's size ratio when the budget is passed, and the highest
/// ratio it is raised to [design].
pub const RATIO_RAISE: f32 = 1.5;
pub const RATIO_MAX: f32 = 0.08;

/// Main-thread time a frame spends spawning distant cells, the bookkeeping included
/// [engineering choice: two thirds of the ring's; never waited for].
pub const SPAWN_BUDGET: Duration = Duration::from_millis(1);

/// Cells after a waiting one whose models are asked of the worker threads meanwhile.
pub const PREFETCH_CELLS: usize = 4;

/// Cell lists kept this many steps beyond the reach (a step back and forth reads none
/// again).
pub const KEEP_LISTS: i32 = 2;

/// Cell lists read on the worker threads at once (the loaded cells' read-ahead shares the
/// pool) [engineering choice].
pub const MAX_LISTING: usize = 8;

/// On a distant cell's entity: the grid square it draws (module notes).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistantCell {
    pub grid: Grid,
}

/// The LOD model of a full model (`meshes\...\name.nif`): `name_lod.nif` beside it when
/// `exists` says so, else `name_lod_0.nif` (one community pack's naming), else none.
pub fn lod_model(full: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let stem = full.strip_suffix(".nif").or_else(|| full.strip_suffix(".NIF"))?;
    if stem.to_ascii_lowercase().ends_with("_lod") {
        return None;
    }
    [format!("{stem}_lod.nif"), format!("{stem}_lod_0.nif")].into_iter().find(|p| exists(p))
}

/// One reference of a cell list, as a worker thread found it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub p: Placement,
    pub base: Arc<BaseObject>,
    /// The full model's archive path.
    pub full: String,
    /// The LOD model's, when the load order has one ([`lod_model`]).
    pub lod: Option<String>,
    /// Visible when distant in any version of the reference or its base.
    pub flagged: bool,
    /// The bound radius (OBND) times the reference's scale; 0 without bounds.
    pub radius: f32,
    /// Enabled at the start of a game.
    pub enabled: bool,
}

/// A cell's references that may be drawn, read on a worker thread.
#[derive(Debug, Default)]
pub struct CellList {
    pub entries: Vec<Entry>,
    /// References left out on the worker: no LOD model and too small for the nearest
    /// distance a distant reference can be at (`inner`).
    pub small: usize,
    /// The inner radius (grid steps) the list was read for.
    pub inner: i32,
}

/// The nearest a reference of a distant cell can be from the loaded grid's middle cell's
/// centre with `inner` the radius it starts beyond (half a cell short of `inner + 1`).
pub fn nearest(inner: i32) -> f32 {
    (inner as f32 + 0.5) * CELL_SIZE
}

/// A base as [`list_cell`] reads it once: the base, its full model and its LOD model;
/// `None` when the scenery filter leaves it out.
type ListedBase = Option<(Arc<BaseObject>, String, Option<String>)>;

/// Whether [`list_cell`] looks at reference `p` at all: not deleted, and drawn with its
/// cell by the renderer's rule ([`world::drawn_with_cell`]: no imposter, no primitive).
fn listed_ref(p: &Placement) -> bool {
    !p.deleted() && world::drawn_with_cell(p)
}

/// Read cell list: the STAT and SCOL references of `refs` that pass the scenery filter
/// (module notes), each with what [`choose`] needs. Runs on a worker thread.
pub fn list_cell(lo: &LoadOrder, schema: &EsmSchema, index: &fnv_world::WorldIndex, refs: &[u32], hide: &[String], inner: i32, exists: impl Fn(&str) -> bool) -> CellList {
    let mut out = CellList { inner, ..default() };
    let mut bases: HashMap<u32, ListedBase> = HashMap::new();
    let small = SIZE_RATIO * HYSTERESIS * nearest(inner);
    for &r in refs {
        let Some(p) = index.placement(lo, schema, r) else { continue };
        if !listed_ref(&p) {
            continue;
        }
        let Some(b) = p.base else { continue };
        let found = bases.entry(b).or_insert_with(|| {
            let base = base_object(lo, schema, b)?;
            if !matches!(&base.sig, b"STAT" | b"SCOL") || base.is_speedtree() {
                return None;
            }
            let full = base.world_model_path()?;
            if world::is_drawn_marker(base.form_id, &full) || hide.iter().any(|h| full.contains(h.as_str())) {
                return None;
            }
            let lod = lod_model(&full, &exists);
            Some((Arc::new(base), full, lod))
        });
        let Some((base, full, lod)) = found.clone() else { continue };
        let flagged = placement_visible_when_distant(lo, &p);
        let radius = base.bound_radius().unwrap_or(0.0) * p.scale;
        if !flagged && lod.is_none() && radius < small {
            out.small += 1;
            continue;
        }
        let enabled = world::enabled(lo, schema, &p, 0);
        out.entries.push(Entry { p, base, full, lod, flagged, radius, enabled });
    }
    out
}

/// What [`choose`] made of a reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Drawn from its LOD model.
    Lod,
    /// Drawn from its full model.
    Full,
    /// Left out: its cell's block holds it.
    Held,
    /// Left out: flagged, and its cell's block is not known yet.
    Unknown,
    /// Left out: no LOD model and too small for its distance (or past the budget).
    Small,
    /// Left out for its state: a model override (destroyed, swapped), or not enabled with
    /// no simulation to show it.
    Skipped,
    /// A full model refused for the triangle budget (not too small): its cell is judged
    /// again when capacity comes back.
    Budget,
}

impl Choice {
    pub fn drawn(self) -> bool {
        matches!(self, Choice::Lod | Choice::Full)
    }
}

/// What [`choose`] reads of a reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Judge {
    pub flagged: bool,
    pub has_lod: bool,
    pub radius: f32,
}

/// The size rule: `radius` at least `ratio` of `distance`, or [`HYSTERESIS`] of that for
/// a model already drawn full; never without a radius.
pub fn size_passes(radius: f32, distance: f32, ratio: f32, was_full: bool) -> bool {
    let need = ratio * distance * if was_full { HYSTERESIS } else { 1.0 };
    radius > 0.0 && radius >= need
}

/// The rule (module notes): `segment` is whether the block of the reference's cell has a
/// non-empty segment for it (`None`: not known yet); `distance` from the loaded grid's
/// middle cell's centre; `was_full`: drawn full before ([`HYSTERESIS`]).
pub fn choose(j: Judge, segment: Option<bool>, distance: f32, ratio: f32, was_full: bool) -> Choice {
    if j.flagged {
        match segment {
            None => return Choice::Unknown,
            Some(true) => return Choice::Held,
            Some(false) => {}
        }
    }
    if j.has_lod {
        Choice::Lod
    } else if size_passes(j.radius, distance, ratio, was_full) {
        Choice::Full
    } else {
        Choice::Small
    }
}

/// A full-model candidate for the budget: its cell's steps from the centre, its radius
/// over its distance (the highest ratio it passes) and its triangles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub steps: i32,
    pub size: f32,
    pub triangles: u64,
}

/// The size ratio of each cell ring (index: grid steps from the centre, up to `reach`)
/// that keeps the candidates' full models within `cap` triangles: `base` everywhere, then
/// raised by [`RATIO_RAISE`] (up to [`RATIO_MAX`]) on the farthest ring that still draws
/// a full model and is below the most, again and again, until the sum fits or no ring can
/// be raised. Also whether any ratio was raised.
pub fn fit_budget(candidates: &[Candidate], reach: i32, base: f32, cap: u64) -> (Vec<f32>, bool) {
    let n = reach.max(0) as usize + 1;
    let mut ratios = vec![base; n];
    // Per ring, sizes sorted from largest, with the running sum of triangles.
    let mut rings: Vec<Vec<(f32, u64)>> = vec![Vec::new(); n];
    for c in candidates.iter().filter(|c| c.steps >= 0 && (c.steps as usize) < n) {
        rings[c.steps as usize].push((c.size, c.triangles));
    }
    for r in &mut rings {
        r.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut sum = 0;
        for e in r.iter_mut() {
            sum += e.1;
            e.1 = sum;
        }
    }
    let at = |ring: &Vec<(f32, u64)>, ratio: f32| -> u64 {
        let k = ring.partition_point(|e| e.0 >= ratio);
        if k == 0 { 0 } else { ring[k - 1].1 }
    };
    let total = |ratios: &[f32]| -> u64 { rings.iter().zip(ratios).map(|(r, &q)| at(r, q)).sum() };
    let mut raised = false;
    while total(&ratios) > cap {
        let Some(i) = (0..n).rev().find(|&i| ratios[i] < RATIO_MAX && at(&rings[i], ratios[i]) > 0) else { break };
        ratios[i] = (ratios[i] * RATIO_RAISE).min(RATIO_MAX);
        raised = true;
    }
    (ratios, raised)
}

/// The size ratio for a cell `s` steps out ([`fit_budget`]'s table, else [`SIZE_RATIO`]).
fn ratio_at(ratios: &[f32], s: i32) -> f32 {
    usize::try_from(s).ok().and_then(|i| ratios.get(i)).copied().unwrap_or(SIZE_RATIO)
}

/// Nearest first, as the ring orders its cells.
fn order_key(c: Grid, centre: Grid) -> i32 {
    steps(c, centre) * 1000 + (c.0 - centre.0).abs() + (c.1 - centre.1).abs()
}

/// The centre of the loaded grid's middle cell (game x, y): distances are taken from it.
pub fn centre_point(centre: Grid) -> Vec2 {
    Vec2::new((centre.0 as f32 + 0.5) * CELL_SIZE, (centre.1 as f32 + 0.5) * CELL_SIZE)
}

/// The distant cells around `centre`: `inner + 1` to `reach` steps out whose nearest point
/// lies within `reach` cell widths of the middle cell's centre (the rest are past the
/// distance fade, [`edge_range`]), nearest first.
pub fn distant_cells(centre: Grid, inner: i32, reach: i32) -> Vec<Grid> {
    let p = centre_point(centre);
    let mut out: Vec<Grid> = (-reach..=reach)
        .flat_map(|dx| (-reach..=reach).map(move |dy| (centre.0 + dx, centre.1 + dy)))
        .filter(|&c| steps(c, centre) > inner && crate::lod::square_distance(p, c, 1) < reach as f32 * CELL_SIZE)
        .collect();
    out.sort_by_key(|&c| order_key(c, centre));
    out
}

/// Which range a distant cell's meshes are drawn with ([`distant_range`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistantRange {
    /// Fading in or out at its own step: each mesh drawn at the smaller of that step and
    /// the reach's fade at its distance ([`band_range`]).
    Own(u8),
    /// Under a loaded or ring cell of the same square: the complement of that cell's step.
    Under(u8),
    /// At rest and alone: fully drawn, dithering out with distance ([`edge_range`]).
    Edge,
}

impl DistantRange {
    /// The range of a mesh at distance `d` from the camera.
    pub fn range(self, d: f32, reach: i32) -> VisibilityRange {
        match self {
            DistantRange::Own(k) => band_range(k, d, reach),
            DistantRange::Under(k) => under_range(k, d, reach),
            DistantRange::Edge => edge_range(reach),
        }
    }

    /// The step of the cell's own pixels, for the old cell a rebuilt one replaces (drawn as
    /// its complement): under a cover or at rest, all of them.
    fn own_step(self) -> u8 {
        match self {
            DistantRange::Own(k) => k,
            DistantRange::Under(_) | DistantRange::Edge => fade::STEPS,
        }
    }
}

/// The range for a distant cell at `own` (0 … 1) whose square has a loaded or ring cell
/// drawn at `cover` (the larger of their scenery levels, [`scenery_level`]), if any.
pub fn distant_range(own: f32, cover: Option<f32>) -> DistantRange {
    match cover {
        Some(l) => DistantRange::Under(fade::step(l)),
        None => match fade::step(own) {
            k if k >= fade::STEPS => DistantRange::Edge,
            k => DistantRange::Own(k),
        },
    }
}

/// How much of cell `c`'s static scenery the loaded cells and the ring draw: the loaded
/// cell's level (`loaded`) or the ring cell's, whichever is more. Not the ring's tree
/// level: a ring cell whose trees fall back to the billboards still draws its statics
/// (`full_detail::tree_level` would read it as absent and let a distant cell draw over it).
pub fn scenery_level(loaded: f32, ring: &HashMap<Grid, full_detail::RingCellState>, c: Grid) -> f32 {
    loaded.max(ring.get(&c).map_or(0.0, |s| s.level))
}

/// The reach's fade band: fully drawn to `reach - 1` cell widths from the camera, nothing
/// from `reach` (game units).
fn edge_band(reach: i32) -> (f32, f32) {
    let inner = (reach - 1).max(1) as f32;
    (inner * CELL_SIZE, (inner + 1.0) * CELL_SIZE)
}

/// Fully drawn within `reach - 1` cell widths of the camera, dithering out to nothing at
/// `reach`.
pub fn edge_range(reach: i32) -> VisibilityRange {
    let (z, w) = edge_band(reach);
    fade::drawn_within(Some(z), w / z - 1.0)
}

/// The dither steps (of [`fade::STEPS`]) the reach's fade leaves drawn at distance `d`, as
/// the visibility-range dither rounds them.
pub fn edge_steps(d: f32, reach: i32) -> u8 {
    let (z, w) = edge_band(reach);
    if d < z {
        return fade::STEPS;
    }
    let hidden = (((d - z) / (w - z)) * fade::STEPS as f32).round().clamp(0.0, fade::STEPS as f32) as u8;
    fade::STEPS - hidden
}

/// A fading distant mesh at distance `d`: drawn at the smaller of its cell's step `k` and
/// the reach's fade there ([`edge_steps`]), on the range's near side; the far side is the
/// empty margin `w..w` at the reach, so from the reach on the mesh is culled outright
/// (`VisibilityRange::is_visible_at_all` is false from `end_margin.end`) and no far-side
/// dither ever draws it. One range cannot multiply the two dithers, so the smaller level
/// stands for their product: inside the band the step alone, at rest the reach's fade
/// alone, never anything beyond the reach.
pub fn band_range(k: u8, d: f32, reach: i32) -> VisibilityRange {
    let (_, w) = edge_band(reach);
    VisibilityRange { start_margin: fade::drawn(k.min(edge_steps(d, reach))).start_margin, end_margin: w..w, use_aabb: false }
}

/// A mesh under a loaded or ring cell at step `k`, at distance `d`: the complement of that
/// cell's step, as the hand-off needs, short of the reach's fade band (where a cover
/// stands in practice: within the ring's reach). Should the camera have moved away so far
/// that the mesh lies in the band, it keeps the complementary pattern (the thresholds the
/// cover leaves out) with its share capped at the reach's fade: `complement(j)` keeps
/// thresholds below `16 - j`, so the larger `j` keeps a subset of them and never one the
/// cover draws. From the reach on nothing (culled outright). The range depends on `d`, so
/// [`fade_distant`] gives it again while the camera may carry meshes across the band
/// ([`cell_zone`]).
pub fn under_range(k: u8, d: f32, reach: i32) -> VisibilityRange {
    let (_, w) = edge_band(reach);
    if d >= w {
        return VisibilityRange { start_margin: 0.0..0.0, end_margin: 0.0..0.0, use_aabb: false };
    }
    let k = k.min(fade::STEPS);
    fade::complement(k.max(fade::STEPS - edge_steps(d, reach)))
}

/// Where a distant cell's meshes lie against the reach's fade band seen from `eye` (game
/// space), from `extent`, the box of their positions (the points a mesh's range is measured
/// from) as last composed: 0 all short of the band, 2 all at or beyond the reach, 1 across
/// it, or not known (no box yet). The box is the meshes' own, not the cell's grid square:
/// a tall or offset mesh (a part of a large collection) stands where it stands. In zones
/// 0 and 2 a distance-dependent range ([`band_range`], [`under_range`]) is the same for
/// every mesh wherever the eye is; it is given again when the zone changes, and in zone 1
/// as the eye moves ([`regive`]).
pub fn cell_zone(extent: Option<(Vec3, Vec3)>, eye: Vec3, reach: i32) -> u8 {
    let Some((lo, hi)) = extent else { return 1 };
    let (z, w) = edge_band(reach);
    let near = eye.clamp(lo, hi).distance(eye);
    let far = (eye - lo).abs().max((eye - hi).abs()).length();
    if far < z {
        0
    } else if near >= w {
        2
    } else {
        1
    }
}

/// How far the eye moves before the distance-dependent ranges of a cell across the reach's
/// fade band are given again: a quarter of one dither step of the band (a cell width over
/// [`fade::STEPS`], 256 units) [engineering choice]. Meanwhile a mesh's level is at most
/// one step (1/16) behind, and stays so while the eye rests within that distance (an eye
/// stopping just past a rounding threshold keeps the step it was given until it moves on:
/// accepted, no re-walk at rest); within 128 units of either edge of the band the rounding gives the
/// same level, and a mesh under a cover keeps a complementary pattern at any level
/// ([`under_range`]), so nothing is ever drawn twice or past the reach.
pub const EYE_STEP: f32 = 64.0;

/// Whether the eye moved more than [`EYE_STEP`] (or came or went) between `was` and `now`.
fn eye_moved(was: Option<Vec3>, now: Option<Vec3>) -> bool {
    match (was, now) {
        (Some(a), Some(b)) => a.distance(b) > EYE_STEP,
        (None, None) => false,
        _ => true,
    }
}

/// Whether a distant cell's meshes get their range again ([`fade_distant`]): never given,
/// or given for another range, reach or zone ([`cell_zone`]), or, across the band (zone
/// 1), for an eye more than [`EYE_STEP`] from `eye` now. A still eye with the same range
/// walks no mesh.
fn regive(applied: Option<Applied>, range: DistantRange, reach: i32, zone: u8, eye: Option<Vec3>) -> bool {
    let Some(a) = applied else { return true };
    a.range != range || a.reach != reach || a.zone != zone || (zone == 1 && eye_moved(a.eye, eye))
}

/// Whether a rebuilt cell `c` may crossfade with the one it replaces: the whole cell lies
/// within `reach - 2` cell widths of the middle cell's centre, so with the camera anywhere
/// in its cell the old cell (drawn as a complement, which cannot carry the reach's fade)
/// is short of the reach's fade band. Otherwise the new cell takes over at once; a
/// predecessor that no longer passes (the reach shrank, the centre moved) goes at once.
pub fn crossfade_ok(c: Grid, centre: Grid, reach: i32) -> bool {
    let p = centre_point(centre);
    let far = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|&(dx, dy)| Vec2::new((c.0 + dx) as f32 * CELL_SIZE, (c.1 + dy) as f32 * CELL_SIZE).distance(p)).fold(0.0, f32::max);
    far <= (reach - 2).max(0) as f32 * CELL_SIZE
}

/// The cell a rebuilt one replaces, still drawn as its complement, with the triangles of
/// each of its full models (still drawn, still counted).
#[derive(Debug)]
struct Old {
    entity: Entity,
    full_of: HashMap<u32, u64>,
    /// The references it draws, from their LOD or full models.
    drawn: HashSet<u32>,
}

impl Old {
    fn share(&self) -> u64 {
        self.full_of.values().sum()
    }
}

/// One drawn distant cell.
#[derive(Debug)]
pub struct CellState {
    pub entity: Entity,
    /// 0 hidden … 1 fully drawn (its own fade).
    pub level: f32,
    /// Fading out, to be retired at 0.
    pub leaving: bool,
    /// A loaded or ring cell was over it last frame (`full_detail::hand_off`).
    under: bool,
    /// What it drew of each reference.
    plan: HashMap<u32, Choice>,
    /// What its plan was judged for, and its block's answer then.
    stamp: Stamp,
    segment: Option<bool>,
    /// It holds flagged references (a block becoming known may change it).
    flagged: bool,
    /// The model override of one of its references came or went: judged again.
    dirty: bool,
    /// The triangles of each of its full models ([`Budget`]).
    full_of: HashMap<u32, u64>,
    /// The smallest full model refused for the budget, if any ([`Choice::Budget`]).
    refused_min: Option<u64>,
    /// The cell it replaces, until this one rests.
    old: Option<Old>,
    /// What its meshes' ranges were last given for ([`fade_distant`]).
    applied: Option<Applied>,
    old_applied: Option<u8>,
    /// The box (game space) of its meshes' positions as last composed for a
    /// distance-dependent range ([`cell_zone`]); `None` before that (or without meshes).
    extent: Option<(Vec3, Vec3)>,
}

/// What a distant cell's meshes' ranges were last given for ([`regive`]): the range, the
/// reach, the cell's zone against the reach's fade band, and the eye a distance-dependent
/// range was measured from (`None` for one that is not).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Applied {
    range: DistantRange,
    reach: i32,
    zone: u8,
    eye: Option<Vec3>,
}

impl CellState {
    fn full_tris(&self) -> u64 {
        self.full_of.values().sum()
    }

    /// A full model of it was refused for the budget ([`Choice::Budget`]).
    fn refused(&self) -> bool {
        self.refused_min.is_some()
    }
}

/// A distant cell whose references are being spawned (hidden until all are in).
struct CellSpawn {
    entity: Entity,
    grid: Grid,
    /// It replaces the drawn cell of its square (a re-judgement).
    replaces: bool,
    list: Arc<CellList>,
    plan: Vec<(usize, Choice)>,
    next: usize,
    /// References whose model was released since the cell's models were found prepared.
    later: Vec<(usize, Choice)>,
    origin: Vec3,
    stamp: Stamp,
    segment: Option<bool>,
    /// What was drawn of each reference so far.
    drawn: HashMap<u32, Choice>,
    /// The triangles of each full model it has drawn so far (its reservation).
    full_of: HashMap<u32, u64>,
    /// The smallest full model refused for the budget, if any.
    refused_min: Option<u64>,
    /// An override of one of its references changed while it was spawned.
    dirty: bool,
}

impl CellSpawn {
    fn done(&self) -> bool {
        self.next >= self.plan.len() && self.later.is_empty()
    }
}

/// What the distant objects did over the run, for the exit summary.
#[derive(Default, Debug, Clone, Copy)]
pub struct DistantStats {
    pub cells: usize,
    pub lod: usize,
    pub full: usize,
    pub held: usize,
    pub unknown: usize,
    pub small: usize,
    /// Full models refused for the triangle budget.
    pub refused: usize,
    pub ms: f64,
}

/// What a settled frame compares to tell that nothing changed ([`stream`]): the inputs the
/// cells are judged and faded from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Settle {
    centre: Option<Grid>,
    inner: i32,
    reach: i32,
    lod: bool,
    world: u32,
    travels: u64,
    loaded: usize,
    loaded_fading: bool,
    streaming: bool,
    ring_cells: usize,
    ring_moving: bool,
    overrides: usize,
    blocks: u64,
}

/// A budget fit running on a worker ([`fit_budget`]'s answer), with the centre and reach
/// it is for.
type Fitting = (Task<(Vec<f32>, bool)>, Grid, i32);

/// The distant cells (module notes).
#[derive(Resource, Default)]
pub struct DistantState {
    /// Grid steps reached (0: off).
    pub reach: i32,
    /// The last reach that was on: cells fading out after Off keep its distance fade.
    fade_reach: i32,
    world: u32,
    travels: u64,
    pub cells: HashMap<Grid, CellState>,
    spawning: HashMap<Grid, CellSpawn>,
    /// Drawn cells to build again (re-judged).
    rebuild: HashSet<Grid>,
    dying: Vec<Entity>,
    listing: HashMap<Grid, Task<CellList>>,
    listed: HashMap<Grid, Arc<CellList>>,
    /// Lists came in since the budget was last fitted.
    lists_changed: bool,
    /// The budget's fit wanted (the centre moved, lists came in) and the one running on a
    /// worker, with the centre and reach it is for.
    fit_wanted: bool,
    fitting: Option<Fitting>,
    /// The centre the size rule was last judged for, and its ratios per ring.
    judged: Option<Grid>,
    ratios: Vec<f32>,
    /// Counts the changes of `ratios` (a cell's [`Stamp`] names the table it was judged
    /// with).
    ratio_rev: u64,
    /// The references with a model override last seen ([`override_changes`]).
    overrides_seen: HashSet<u32>,
    /// The centre the budget's raise was last logged for.
    budget_logged: Option<Grid>,
    /// Triangles of each full model drawn so far, by path.
    tris_of: HashMap<String, u64>,
    /// The one rebuilt cell allowed to count without its predecessor's share ([`admits`]).
    credit: Option<Grid>,
    /// The cells in a stable order and where the re-judge scan goes on from
    /// ([`queue_rebuilds`]); `cells_rev` counts the changes of the cell set.
    scan: Vec<Grid>,
    scan_rev: u64,
    scan_at: usize,
    /// What the scan inspects for (block revision, stamp, reach, cell set) and how many
    /// cells it has inspected for it: a lap is complete once every cell has been.
    scan_for: Option<ScanKey>,
    scan_seen: usize,
    cells_rev: u64,
    /// The cached cells wanted, for this centre, inner radius and reach.
    want: Vec<Grid>,
    want_for: Option<(Grid, i32, i32)>,
    /// Nothing is under way and the inputs were `settled` last frame: a settled frame
    /// returns at once ([`stream`]); `fade_dirty`: the ranges may need giving again.
    quiet: bool,
    settled: Settle,
    fade_dirty: bool,
    /// Some cell's range depends on the eye's distance (fading or covered), and the eye at
    /// [`fade_distant`]'s last pass: while one does, a moved eye is a reason for a pass.
    watch: bool,
    eye_at: Option<Vec3>,
    /// The off line was logged since the row went Off.
    off_logged: bool,
    /// `world::spawn_model`'s counts for the distant models.
    models: StreamStats,
    pub stats: DistantStats,
    /// The last reach that was on (the exit summary is logged only then).
    used: Option<i32>,
}

impl DistantState {
    fn idle(&self) -> bool {
        self.reach == 0 && self.cells.is_empty() && self.spawning.is_empty() && self.dying.is_empty() && self.listing.is_empty() && self.listed.is_empty()
    }

    /// Drop every cell, spawn and reading: `at_once` (a travel, another worldspace: behind
    /// the loading screen) despawns them now; otherwise (indoors, the LOD off) they are
    /// hidden now and despawned over the next frames in bounded slices.
    fn clear(&mut self, commands: &mut Commands, at_once: bool) {
        let olds: Vec<Entity> = self.cells.values().filter_map(|s| s.old.as_ref().map(|o| o.entity)).collect();
        let all: Vec<Entity> = self.cells.drain().map(|(_, s)| s.entity).chain(self.spawning.drain().map(|(_, s)| s.entity)).chain(olds).collect();
        if at_once {
            for e in all.into_iter().chain(self.dying.drain(..)) {
                commands.entity(e).try_despawn();
            }
        } else {
            for e in all {
                self.retire(commands, e);
            }
        }
        self.fitting = None;
        self.fit_wanted = false;
        self.rebuild.clear();
        self.listing.clear();
        self.listed.clear();
        self.judged = None;
        self.credit = None;
        self.want_for = None;
        self.cells_rev += 1;
        self.quiet = false;
    }

    /// The triangle budget's parts, read from the cells alive now ([`Budget`]).
    pub fn budget(&self) -> Budget {
        Budget {
            retained: self.cells.values().map(CellState::full_tris).sum(),
            reserved: self.spawning.values().map(|s| s.full_of.values().sum::<u64>()).sum(),
            fading: self.cells.values().filter_map(|s| s.old.as_ref().map(Old::share)).sum(),
        }
    }

    /// Hide a cell's entity and despawn it over the next frames.
    fn retire(&mut self, commands: &mut Commands, e: Entity) {
        commands.entity(e).insert(Visibility::Hidden);
        self.dying.push(e);
    }

    /// Retire a drawn cell with the one it replaces.
    fn retire_cell(&mut self, commands: &mut Commands, s: CellState) {
        if let Some(o) = s.old {
            self.retire(commands, o.entity);
        }
        self.retire(commands, s.entity);
        self.cells_rev += 1;
    }

    /// The rebuilt cell holding the predecessor credit, if it still needs it: a replacement
    /// being spawned, or one crossfading with its predecessor.
    fn release_credit(&mut self) {
        if let Some(c) = self.credit {
            let spawning = self.spawning.get(&c).is_some_and(|s| s.replaces);
            let fading = self.cells.get(&c).is_some_and(|s| s.old.is_some());
            if !spawning && !fading {
                self.credit = None;
            }
        }
    }
}

/// What a re-judge scan's lap is for: the blocks' revision, the stamp judged now, the
/// reach and the cell set. A lap counts as complete only while all four stay the same.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ScanKey {
    blocks: u64,
    now: Stamp,
    reach: i32,
    cells: u64,
}

/// What a cell's plan was judged for: the loaded grid's centre and the revision of the
/// ratio table. A cell whose stamp differs from the current one is judged again once it
/// is at rest and alone ([`queue_rebuilds`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub centre: Grid,
    pub ratios: u64,
}

/// The full models' triangles, in three parts (module notes): those of the drawn cells
/// (`retained`), those cells being spawned have drawn so far (`reserved`) and those of
/// the cells a rebuilt cell replaces while they fade out (`fading`). Each part is read
/// from the cells alive, never counted up and down.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub retained: u64,
    pub reserved: u64,
    pub fading: u64,
}

impl Budget {
    pub fn total(self) -> u64 {
        self.retained + self.reserved + self.fading
    }
}

/// Whether a full model of `tris` triangles may be drawn by a spawn within `cap`: against
/// everything drawn, reserved and fading, except, for the one rebuilt cell holding the
/// predecessor credit (`DistantState::credit`), its `predecessor`'s retained models (they
/// go once it rests; counting them would drop models the new cell keeps). So the total
/// passes the cap by at most one cell's share (that predecessor's), from the
/// replacement's first full model until the predecessor has faded out; every other
/// spawn counts everything [design].
pub fn admits(b: Budget, predecessor: u64, tris: u64, cap: u64) -> bool {
    b.retained.saturating_sub(predecessor) + b.reserved + b.fading + tris <= cap
}

/// The references whose model override came or went between `seen` and `now`.
pub fn override_changes(seen: &HashSet<u32>, now: &HashMap<u32, String>) -> HashSet<u32> {
    let mut out: HashSet<u32> = seen.iter().filter(|r| !now.contains_key(r)).copied().collect();
    out.extend(now.keys().filter(|r| !seen.contains(r)));
    out
}

/// The plan of cell `c` from its list: each entry's [`Choice`] (module notes); `skip`
/// leaves an entry out for its state ([`Choice::Skipped`]).
fn plan_cell(list: &CellList, c: Grid, centre: Grid, ratios: &[f32], segment: Option<bool>, before: Option<&HashMap<u32, Choice>>, skip: &dyn Fn(&Entry) -> bool) -> Vec<(usize, Choice)> {
    let p = centre_point(centre);
    let ratio = ratio_at(ratios, steps(c, centre));
    list.entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let d = Vec2::new(e.p.position[0], e.p.position[1]).distance(p);
            if skip(e) {
                return (i, Choice::Skipped);
            }
            let was_full = before.and_then(|b| b.get(&e.p.form_id)) == Some(&Choice::Full);
            (i, choose(Judge { flagged: e.flagged, has_lod: e.lod.is_some(), radius: e.radius }, segment, d, ratio, was_full))
        })
        .collect()
}

/// The drawn part of a plan (reference, choice), to tell whether a cell must be built
/// again. A model refused for the budget counts as the full model it was planned as, so a
/// cell is not rebuilt for that alone (capacity coming back marks it, [`stream`]).
fn drawn_set(list: &CellList, plan: &[(usize, Choice)]) -> BTreeMap<u32, Choice> {
    plan.iter().filter(|(_, ch)| ch.drawn()).map(|&(i, ch)| (list.entries[i].p.form_id, ch)).collect()
}

/// [`drawn_set`] of what a drawn cell did draw.
fn drawn_of(plan: &HashMap<u32, Choice>) -> BTreeMap<u32, Choice> {
    plan.iter().filter(|(_, ch)| ch.drawn()).map(|(r, ch)| (*r, *ch)).collect()
}

/// The model (path, alternate textures) a planned reference draws.
fn model_of(e: &Entry, ch: Choice) -> Option<(&str, &[AltTexture])> {
    match ch {
        Choice::Lod => e.lod.as_deref().map(|l| (l, &[][..])),
        Choice::Full => Some((e.full.as_str(), e.base.world_alt_textures.as_slice())),
        _ => None,
    }
}

/// Whether a rebuilt cell's predecessor goes now: the new cell is at rest, under a cover
/// or leaving, or the predecessor may no longer crossfade ([`crossfade_ok`]: the reach
/// shrank or the centre moved), when the new cell takes over at once.
fn predecessor_goes(level: f32, covered: bool, leaving: bool, crossfade: bool) -> bool {
    level >= 1.0 || covered || leaving || !crossfade
}

/// The distant cells' streaming each frame (module notes); after the ring, before
/// [`fade_distant`]. A settled frame (nothing under way, the inputs unchanged) returns
/// after comparing them.
#[allow(clippy::too_many_arguments)]
pub fn stream(
    mut commands: Commands,
    world: Res<WorldData>,
    graphics: Res<Graphics>,
    streamer: Res<Streamer>,
    (ring, lod, blocks): (Res<FullDetailRing>, Res<LodState>, Res<BlockContents>),
    mut state: ResMut<DistantState>,
    root: Query<Entity, With<WorldRoot>>,
    time: Res<Time>,
    mut models: ModelCtx,
    mut emittance: ResMut<EmittanceState>,
    (sim, loading): (Option<Res<nvf_play::sim::SimState>>, Option<Res<nvf_play::loading::Loading>>),
    (children, refs): (Query<&Children>, Query<&nvf_play::RefVisual>),
) {
    let st = &mut *state;
    let reach = if lod.enabled { graphics.distant_objects.reach() } else { 0 };
    if reach == 0 && st.idle() {
        return;
    }
    let started = Instant::now();
    let centre = streamer.centre.filter(|_| streamer.interior.is_none());
    // Indoors (or the distant LOD off) with everything already gone: nothing to clean up.
    let empty = st.cells.is_empty() && st.spawning.is_empty() && st.dying.is_empty() && st.listing.is_empty() && st.listed.is_empty() && st.fitting.is_none();
    if (centre.is_none() || !lod.enabled) && empty && st.world == world.world && st.travels == streamer.travels {
        return;
    }
    let inner = streamer.radius.max(graphics.full_detail.radius());
    let settle = Settle {
        centre,
        inner,
        reach,
        lod: lod.enabled,
        world: world.world,
        travels: streamer.travels,
        loaded: streamer.loaded.len(),
        loaded_fading: !streamer.fades.is_empty(),
        streaming: !streamer.settled || !streamer.spawning.is_empty(),
        ring_cells: ring.cells.len(),
        ring_moving: ring.cells.values().any(|s| !s.full()),
        overrides: streamer.model_overrides.len(),
        blocks: blocks.rev(),
    };
    // Model overrides that came or went (one pass over the overrides, every frame).
    let changed = override_changes(&st.overrides_seen, &streamer.model_overrides);
    if st.quiet && st.settled == settle && changed.is_empty() {
        st.stats.ms += started.elapsed().as_secs_f64() * 1000.0;
        return;
    }
    st.settled = settle;
    st.quiet = false;
    st.fade_dirty = true;
    world::despawn_dying(&mut commands, &mut st.dying, &children);
    let travelled = st.world != world.world || st.travels != streamer.travels;
    if travelled || centre.is_none() || !lod.enabled {
        st.clear(&mut commands, travelled);
        st.world = world.world;
        st.travels = streamer.travels;
    }
    let shrank = reach < st.reach;
    if reach != st.reach {
        st.reach = reach;
        if reach > 0 {
            info!("distant objects: reach {reach} cells (ratio {SIZE_RATIO})");
            st.used = Some(reach);
            st.fade_reach = reach;
            st.off_logged = false;
        } else {
            st.listing.clear();
            st.listed.clear();
        }
        st.judged = None;
    }
    let Some(centre) = centre else { return };
    let Ok(root) = root.single() else { return };
    let fading = streamer.fade_secs > 0.0;
    let cover = |c: &Grid| (streamer.loaded.contains_key(c) || ring.cells.contains_key(c)).then(|| scenery_level(streamer.cell_level(*c), &ring.cells, *c));
    let cover_full = |c: &Grid| (streamer.loaded.contains_key(c) && !streamer.fades.contains_key(c)) || ring.cells.get(c).is_some_and(|s| s.full());

    // The shapes of a reference whose override came go at once from every distant cell
    // holding it; the cells holding a changed one are judged again.
    if !changed.is_empty() {
        st.overrides_seen = streamer.model_overrides.keys().copied().collect();
        let came: HashSet<u32> = changed.iter().copied().filter(|r| streamer.model_overrides.contains_key(r)).collect();
        hide_overridden(&mut commands, st, &came, &children, &refs);
        mark_overridden(st, &changed);
    }

    if reach > 0 && st.want_for != Some((centre, inner, reach)) {
        st.want = distant_cells(centre, inner, reach);
        st.want_for = Some((centre, inner, reach));
    }
    let want = if reach > 0 { std::mem::take(&mut st.want) } else { Vec::new() };
    let frame = CellFrame {
        centre,
        inner,
        reach,
        shrank,
        fading,
        rate: if fading { time.delta_secs() / streamer.fade_secs } else { 1.0 },
        clamp_reach: if reach > 0 { reach } else { st.fade_reach },
    };
    advance_cells(&mut commands, st, &frame, &want, &cover, &cover_full);
    if reach == 0 {
        // Off: the cells fade out through the frames above; one line once the last is gone.
        if st.cells.is_empty() && st.spawning.is_empty() && st.used.is_some() && !st.off_logged {
            st.off_logged = true;
            info!("distant objects: off, 0 cells left");
        }
        st.stats.ms += started.elapsed().as_secs_f64() * 1000.0;
        return;
    }

    let deadline = started + SPAWN_BUDGET;
    let behind_screen = loading.is_some_and(|l| l.unseen_frames());
    let loaded = streamer.radius;
    let complete = (-loaded..=loaded).all(|dx| (-loaded..=loaded).all(|dy| streamer.loaded.contains_key(&(centre.0 + dx, centre.1 + dy))));
    let idle = streamer.settled && complete && streamer.spawning.is_empty() && !behind_screen && ring.idle(centre, loaded);
    list_cells(&world, st, &want, centre, inner, idle && !streamer.ahead.busy(), &streamer.hide, &models.data.vfs, deadline);
    if !idle {
        st.want = want;
        st.stats.ms += started.elapsed().as_secs_f64() * 1000.0;
        return;
    }

    // The size rule's ratios, fitted to the budget on a worker when the centre moved or
    // lists came in; a finished fit is taken in only while it is for the centre and reach
    // judged now (a stale one is dropped and fitted again).
    // The centre this frame judges for first, then the fit compared with it.
    if st.judged.is_none_or(|j| steps(j, centre) >= REJUDGE_STEPS) {
        st.judged = Some(centre);
        st.fit_wanted = true;
    }
    let fitted = st.fitting.as_mut().and_then(|(task, at, r)| check_ready(task).map(|res| (res, *at, *r)));
    if let Some(((ratios, raised), at, r)) = fitted {
        st.fitting = None;
        if fit_current(at, r, st.judged, st.reach) {
            take_fit(st, ratios, raised, at);
        } else {
            st.fit_wanted = true;
        }
    }
    if st.lists_changed {
        st.lists_changed = false;
        st.fit_wanted = true;
    }
    if st.fit_wanted && st.fitting.is_none() {
        st.fit_wanted = false;
        let at = st.judged.unwrap_or(centre);
        start_fit(st, &want, at, &blocks);
    }
    // Room for a refused model (a cell or a predecessor went): judged again.
    retry_refused(st);
    let judged = st.judged.unwrap_or(centre);
    let now = Stamp { centre: judged, ratios: st.ratio_rev };
    // Cells judged for another centre or table, or whose block answers otherwise now; the
    // size rule waits for the fit of a new centre, so a cell is not judged twice.
    let scan = st.fitting.is_none();
    rejudge(&mut commands, st, now, &blocks, &cover, &want, fading, scan, deadline);
    // The budget read after the re-judge step: a cell it retired at once (fades off) has
    // given its share back before a spawn checks capacity.
    let mut budget = st.budget();

    let ctx = SpawnCtx { world: &world, streamer: &streamer, simulated: sim.is_some() };
    let mut under_way: Vec<Grid> = st.spawning.keys().copied().collect();
    under_way.sort_by_key(|&c| order_key(c, centre));
    for c in under_way {
        if Instant::now() >= deadline {
            break;
        }
        let Some(mut sp) = st.spawning.remove(&c) else { continue };
        spawn_refs(&mut commands, &ctx, st, &mut sp, &mut budget, deadline, &mut models, &mut emittance);
        if sp.done() {
            finish(&mut commands, st, c, sp, fading, crossfade_ok(c, centre, reach));
            budget = st.budget();
        } else {
            st.spawning.insert(c, sp);
        }
    }
    // Cells not drawn yet nearest first, then cells to build again.
    let mut todo: Vec<(Grid, bool)> = want.iter().filter(|c| !st.cells.contains_key(c) && !st.spawning.contains_key(c)).map(|&c| (c, false)).collect();
    todo.sort_by_key(|&(c, _)| (cover(&c).is_some(), order_key(c, centre)));
    let mut again: Vec<Grid> = st.rebuild.iter().filter(|c| !st.spawning.contains_key(c)).copied().collect();
    again.sort_by_key(|&c| order_key(c, centre));
    todo.extend(again.into_iter().map(|c| (c, true)));
    let simulated = sim.is_some();
    let skip = |e: &Entry| streamer.model_overrides.contains_key(&e.p.form_id) || !full_detail::drawn_for_enable_state(e.enabled, e.p.deleted(), simulated);
    let mut waiting = 0usize;
    let mut stopped = false;
    for (c, replaces) in todo {
        if Instant::now() >= deadline || waiting >= PREFETCH_CELLS {
            stopped = true;
            break;
        }
        // Nearest first: a cell waits for its list.
        let Some(list) = st.listed.get(&c).cloned() else {
            if replaces {
                // A queued cell without a list ([`rejudge`] let those go before; kept so a
                // rebuild entry never outlives its list).
                st.rebuild.remove(&c);
                continue;
            }
            stopped = true;
            break;
        };
        let segment = blocks.segment(c);
        let before = if replaces { st.cells.get(&c).map(|s| &s.plan) } else { None };
        let plan = plan_cell(&list, c, judged, &st.ratios, segment, before, &skip);
        if replaces {
            st.rebuild.remove(&c);
            // The same drawn set, and no model refused for the budget that could go in now.
            let same = st.cells.get(&c).is_some_and(|s| drawn_set(&list, &plan) == drawn_of(&s.plan) && !s.refused());
            if same {
                if let Some(s) = st.cells.get_mut(&c) {
                    s.stamp = now;
                    s.segment = segment;
                    s.dirty = false;
                }
                continue;
            }
        }
        let ready = models.models_ready(plan.iter().filter_map(|&(i, ch)| model_of(&list.entries[i], ch)));
        if !ready || waiting > 0 {
            // Its models are asked of the worker threads; later cells only ask for theirs.
            waiting += 1;
            if replaces {
                st.rebuild.insert(c);
            }
            continue;
        }
        let origin = cell_origin(c);
        let entity = commands.spawn((Name::new(format!("Distant cell {},{}", c.0, c.1)), DistantCell { grid: c }, nvf_play::DrawOnlyCell, Transform::from_translation(origin), Visibility::Hidden)).id();
        commands.entity(root).add_child(entity);
        let mut sp = CellSpawn { entity, grid: c, replaces, list, plan, next: 0, later: Vec::new(), origin, stamp: now, segment, drawn: HashMap::new(), full_of: HashMap::new(), refused_min: None, dirty: false };
        spawn_refs(&mut commands, &ctx, st, &mut sp, &mut budget, deadline, &mut models, &mut emittance);
        if sp.done() {
            finish(&mut commands, st, c, sp, fading, crossfade_ok(c, centre, reach));
            budget = st.budget();
        } else {
            st.spawning.insert(c, sp);
        }
    }
    st.release_credit();
    // Replacements finished this frame may have freed room after the check above.
    retry_refused(st);
    // Quiet: nothing under way, every cover at rest (a loaded or ring cell still fading
    // changes the ranges every frame), the re-judge scan through a whole lap for the
    // blocks, stamp, reach and cells now, every wanted cell drawn at rest and judged for
    // now, so the next frames with the same inputs need do nothing.
    let busy = stopped || settle.ring_moving || settle.loaded_fading || waiting > 0;
    st.quiet = layer_quiet(st, busy, &want, now, &blocks);
    st.want = want;
    st.stats.ms += started.elapsed().as_secs_f64() * 1000.0;
}

/// A grid square's south-west corner (game space).
fn cell_origin(c: Grid) -> Vec3 {
    Vec3::new(c.0 as f32 * CELL_SIZE, c.1 as f32 * CELL_SIZE, 0.0)
}

/// What a frame's cell lifecycle reads ([`advance_cells`]): the loaded grid's centre, the
/// inner radius and the reach, whether the reach shrank this frame, whether cells fade
/// (and by how much this frame), and the reach whose crossfade rule applies (the last one
/// on, after Off).
#[derive(Debug, Clone, Copy)]
struct CellFrame {
    centre: Grid,
    inner: i32,
    reach: i32,
    shrank: bool,
    fading: bool,
    rate: f32,
    clamp_reach: i32,
}

/// The drawn cells' lifecycle for one frame ([`stream`]): the hand-off with the loaded and
/// ring cells (`cover`: the level of the one over a cell, `cover_full`: it is fully
/// drawn), the cells that start leaving (past the reach's hysteresis step), the wanted
/// ones kept, each cell's fade, the predecessors that go, then the cells and spawns that
/// go, retired (hidden now, despawned over the next frames).
fn advance_cells(commands: &mut Commands, st: &mut DistantState, f: &CellFrame, want: &[Grid], cover: &dyn Fn(&Grid) -> Option<f32>, cover_full: &dyn Fn(&Grid) -> bool) {
    let mut gone: Vec<Grid> = Vec::new();
    for (c, s) in &mut st.cells {
        let k = steps(*c, f.centre);
        // The ring's rule: held at rest under a loaded or ring cell; a leaving cell already
        // under one keeps its complement until that cell rests, one first met while
        // leaving goes at once.
        let under = cover(c).map(|_| cover_full(c));
        match hand_off(s.level, s.leaving, under, s.under) {
            None => gone.push(*c),
            Some(_) if covered(k, f.inner, cover_full(c)) => gone.push(*c),
            Some(level) => {
                s.level = level;
                s.under = under.is_some();
                if fades_out(k, f.reach, f.shrank) {
                    if f.fading {
                        s.leaving = true;
                    } else {
                        gone.push(*c);
                    }
                }
            }
        }
    }
    for c in want {
        if let Some(s) = st.cells.get_mut(c) {
            s.leaving = false;
        }
    }
    let mut rested: Vec<Entity> = Vec::new();
    for (c, s) in &mut st.cells {
        // Held at rest under a cover ([`hand_off`]).
        if !s.under {
            s.level = (s.level + if s.leaving { -f.rate } else { f.rate }).clamp(0.0, 1.0);
            if s.leaving && s.level <= 0.0 {
                gone.push(*c);
            }
        }
        if s.old.is_some() {
            let crossfade = crossfade_ok(*c, f.centre, f.clamp_reach);
            if predecessor_goes(s.level, cover(c).is_some(), s.leaving, crossfade) {
                rested.extend(s.old.take().map(|o| o.entity));
                s.old_applied = None;
                // Past the crossfade's reach: the new cell takes over at once.
                if !crossfade && !s.leaving {
                    s.level = 1.0;
                }
            }
        }
    }
    for e in rested {
        st.retire(commands, e);
    }
    for c in gone {
        if let Some(s) = st.cells.remove(&c) {
            st.retire_cell(commands, s);
        }
        st.rebuild.remove(&c);
        if let Some(sp) = st.spawning.remove(&c) {
            st.retire(commands, sp.entity);
        }
    }
    let dropped: Vec<Grid> = st.spawning.keys().filter(|&&c| fades_out(steps(c, f.centre), f.reach, f.shrank) || steps(c, f.centre) <= f.inner).copied().collect();
    for c in dropped {
        if let Some(sp) = st.spawning.remove(&c) {
            st.retire(commands, sp.entity);
        }
    }
    st.release_credit();
}

/// The cells to judge again this frame ([`stream`]): the stale ones queued
/// ([`queue_rebuilds`], only with `scan`: the size rule waits for a new centre's fit), then
/// the queued cells whose list is gone let go of (none being replaced): one outside the
/// wanted cells (the hysteresis step, after the inner radius shrank) cannot be judged
/// again, so it goes ([`drop_unlisted`]) instead of being queued by every scan and dropped
/// by every rebuild, which kept the layer from ever going quiet; a wanted one waits for
/// its list.
#[allow(clippy::too_many_arguments)]
fn rejudge(commands: &mut Commands, st: &mut DistantState, now: Stamp, blocks: &BlockContents, cover: &dyn Fn(&Grid) -> Option<f32>, want: &[Grid], fading: bool, scan: bool, deadline: Instant) {
    if scan {
        queue_rebuilds(st, now, blocks, cover, deadline);
    }
    let unlisted: Vec<Grid> = st.rebuild.iter().filter(|c| !st.listed.contains_key(c) && !st.spawning.contains_key(c)).copied().collect();
    if unlisted.is_empty() {
        return;
    }
    let wanted: HashSet<Grid> = want.iter().copied().collect();
    for c in unlisted {
        st.rebuild.remove(&c);
        if !wanted.contains(&c) {
            drop_unlisted(commands, st, c, fading);
        }
    }
}

/// Whether the layer can sleep after this frame ([`stream`]): not `busy` (the frame's
/// planning stopped at the deadline or waits for models, a loaded or ring cell still
/// fading changes the ranges every frame), the re-judge scan through a whole lap for the
/// blocks, stamp, reach and cells now, nothing under way (spawns, rebuilds, lists, cells
/// still being despawned, a fit), every wanted cell drawn, and every drawn cell at rest
/// and judged for now ([`cell_settled`]). Cells being despawned only wait for the next
/// frames' bounded slices (`world::despawn_dying`, run by every frame that is not quiet).
fn layer_quiet(st: &DistantState, busy: bool, want: &[Grid], now: Stamp, blocks: &BlockContents) -> bool {
    let scan_key = ScanKey { blocks: blocks.rev(), now, reach: st.reach, cells: st.cells_rev };
    !busy
        && scan_complete(st, scan_key)
        && st.spawning.is_empty()
        && st.rebuild.is_empty()
        && st.listing.is_empty()
        && st.dying.is_empty()
        && st.fitting.is_none()
        && !st.fit_wanted
        && !st.lists_changed
        && want.iter().all(|c| st.cells.contains_key(c))
        && st.cells.iter().all(|(c, s)| cell_settled(s, now, blocks.segment(*c)))
}

/// Start listing the wanted cells not listed (nearest first, [`MAX_LISTING`] at a time,
/// only when `start`); take in the lists done; forget those far beyond the reach or read
/// for a larger inner radius than now.
#[allow(clippy::too_many_arguments)]
fn list_cells(world: &WorldData, st: &mut DistantState, want: &[Grid], centre: Grid, inner: i32, start: bool, hide: &[String], vfs: &Arc<fnv_vfs::Vfs>, deadline: Instant) {
    let keep = st.reach + KEEP_LISTS;
    st.listing.retain(|c, _| steps(*c, centre) <= keep);
    st.listed.retain(|c, l| steps(*c, centre) <= keep && l.inner <= inner);
    let mut done = Vec::new();
    st.listing.retain(|c, task| match check_ready(task) {
        Some(l) => {
            done.push((*c, l));
            false
        }
        None => true,
    });
    for (c, l) in done {
        // Read for a larger inner radius than now: read again.
        if l.inner <= inner {
            st.listed.insert(c, Arc::new(l));
            st.lists_changed = true;
        }
    }
    if !start {
        return;
    }
    for &c in want {
        if st.listing.len() >= MAX_LISTING || Instant::now() >= deadline {
            break;
        }
        if st.listing.contains_key(&c) || st.listed.contains_key(&c) {
            continue;
        }
        let mut refs: Vec<u32> = world.grid_cell(c).map(|cell| cell.refs.clone()).unwrap_or_default();
        refs.extend(world.persistent_by_grid.get(&c).into_iter().flatten());
        if refs.is_empty() {
            st.listed.insert(c, Arc::new(CellList { inner, ..default() }));
            continue;
        }
        let (lo, schema, index, hide, vfs) = (world.lo, world.schema, world.index, hide.to_vec(), vfs.clone());
        let task = nvf_assets::read_pool::read_pool().spawn(async move { list_cell(lo, schema, index, &refs, &hide, inner, |p| vfs.exists(p)) });
        st.listing.insert(c, task);
    }
}

/// The cells a budget fit reads: each listed wanted cell's steps from the centre, its
/// block's answer and its list.
type FitCells = Vec<(i32, Option<bool>, Arc<CellList>)>;

/// The full-model candidates of `cells` judged from `centre` ([`fit_budget`]): references
/// without a LOD model, with a radius, not left to their block; `tris_of` the triangles of
/// the full models drawn so far ([`UNKNOWN_TRIANGLES`] for the rest). Runs on a worker.
pub fn candidates(cells: &[(i32, Option<bool>, Arc<CellList>)], tris_of: &HashMap<String, u64>, centre: Grid) -> Vec<Candidate> {
    let p = centre_point(centre);
    let mut out = Vec::new();
    for (k, segment, list) in cells {
        for e in list.entries.iter().filter(|e| e.lod.is_none() && e.radius > 0.0 && !(e.flagged && *segment != Some(false))) {
            let d = Vec2::new(e.p.position[0], e.p.position[1]).distance(p).max(1.0);
            let triangles = tris_of.get(&e.full).copied().unwrap_or(UNKNOWN_TRIANGLES);
            out.push(Candidate { steps: *k, size: e.radius / d, triangles });
        }
    }
    out
}

/// Start the budget's fit for `centre` on a worker over the listed wanted cells (the main
/// thread only gathers the cells' lists); the result names the centre and reach it is for.
fn start_fit(st: &mut DistantState, want: &[Grid], centre: Grid, blocks: &BlockContents) {
    let cells: FitCells = want.iter().filter_map(|c| Some((steps(*c, centre), blocks.segment(*c), st.listed.get(c)?.clone()))).collect();
    let (tris_of, reach) = (st.tris_of.clone(), st.reach);
    let task = nvf_assets::read_pool::read_pool().spawn(async move { fit_budget(&candidates(&cells, &tris_of, centre), reach, SIZE_RATIO, FULL_TRIANGLE_BUDGET) });
    st.fitting = Some((task, centre, reach));
}

/// Whether a finished fit for centre `at` and reach `r` is for the centre this frame
/// judges for (`judged`, already advanced) and the reach now.
fn fit_current(at: Grid, r: i32, judged: Option<Grid>, reach: i32) -> bool {
    Some(at) == judged && r == reach
}

/// Take a finished fit (for `centre`) in: new ratios make every cell's stamp stale; log
/// once per centre when the budget bites.
fn take_fit(st: &mut DistantState, ratios: Vec<f32>, raised: bool, centre: Grid) {
    if ratios != st.ratios {
        st.ratio_rev += 1;
    }
    if raised && st.budget_logged != Some(centre) {
        st.budget_logged = Some(centre);
        let from = ratios.iter().position(|&r| r > SIZE_RATIO).unwrap_or(0);
        let most = ratios.iter().copied().fold(SIZE_RATIO, f32::max);
        info!("distant objects: full models past {FULL_TRIANGLE_BUDGET} triangles; size ratio raised from {from} cells out (up to {most:.3})");
    }
    st.ratios = ratios;
}

/// Queue the drawn cells to judge again ([`Stamp`]): those judged for another centre or
/// ratio table than `now`, and those holding flagged references whose block answers
/// otherwise than when they were judged (it became known, empty or not). Only cells at
/// rest, alone and not being replaced: the others keep their stale stamp and are queued
/// once they are (whether the cell is built again is decided when it is planned, by its
/// drawn set). The cells are scanned in a stable order from where the last scan stopped
/// (at the deadline), so every cell is reached however short the frames.
fn queue_rebuilds(st: &mut DistantState, now: Stamp, blocks: &BlockContents, cover: &dyn Fn(&Grid) -> Option<f32>, deadline: Instant) {
    if st.scan_rev != st.cells_rev || st.scan.len() != st.cells.len() {
        st.scan = st.cells.keys().copied().collect();
        st.scan.sort_unstable();
        st.scan_rev = st.cells_rev;
        st.scan_at = 0;
    }
    let key = ScanKey { blocks: blocks.rev(), now, reach: st.reach, cells: st.cells_rev };
    if st.scan_for != Some(key) {
        st.scan_for = Some(key);
        st.scan_seen = 0;
    }
    let (n, start) = (st.scan.len(), st.scan_at);
    let mut queue = Vec::new();
    let mut looked = 0;
    while looked < n {
        if looked % 64 == 63 && Instant::now() >= deadline {
            break;
        }
        let c = st.scan[(start + looked) % n];
        looked += 1;
        let Some(s) = st.cells.get(&c) else { continue };
        if !st.rebuild.contains(&c) && rejudge_due(s, now, blocks.segment(c), cover(&c).is_some(), st.spawning.contains_key(&c)) {
            queue.push(c);
        }
    }
    st.scan_at = scan_next(start, looked, n);
    st.scan_seen += looked;
    st.rebuild.extend(queue);
}

/// Whether the re-judge scan has inspected every cell for `key` (a full lap since the
/// blocks, the stamp, the reach or the cell set last changed).
fn scan_complete(st: &DistantState, key: ScanKey) -> bool {
    st.scan_for == Some(key) && st.scan_seen >= st.cells.len()
}

/// The scan position after looking at `looked` of `n` cells from `at` ([`queue_rebuilds`]).
fn scan_next(at: usize, looked: usize, n: usize) -> usize {
    (at + looked) % n.max(1)
}

/// Let drawn cell `c` go (it is outside the wanted cells and has no list to be judged
/// with): it fades out (at once with fades off) and is retired as a leaving cell is.
fn drop_unlisted(commands: &mut Commands, st: &mut DistantState, c: Grid, fading: bool) {
    if fading {
        if let Some(s) = st.cells.get_mut(&c) {
            s.leaving = true;
        }
    } else if let Some(s) = st.cells.remove(&c) {
        st.retire_cell(commands, s);
    }
}

/// Whether a drawn cell lets the layer sleep: at rest (or held under a cover), not
/// leaving, replacing or marked, judged for `now`, and, holding flagged references, judged
/// with the block answer it has now (`segment`). A cell the scan passed while it was
/// skipped (fading, covered, replacing) and whose block changed meanwhile keeps the state
/// awake until the scan comes round to it again.
fn cell_settled(s: &CellState, now: Stamp, segment: Option<bool>) -> bool {
    (s.level >= 1.0 || s.under) && !s.leaving && s.old.is_none() && !s.dirty && (s.under || (s.stamp == now && (!s.flagged || s.segment == segment)))
}

/// Whether a drawn cell is to be judged again now ([`queue_rebuilds`]): `segment` is its
/// block's answer now, `covered` a loaded or ring cell over it, `spawning` a replacement
/// under way. A cell whose references' overrides changed, or with a model refused for
/// the budget when capacity came back, goes even while fading or covered.
fn rejudge_due(s: &CellState, now: Stamp, segment: Option<bool>, covered: bool, spawning: bool) -> bool {
    if s.leaving || s.old.is_some() || spawning {
        return false;
    }
    if s.dirty {
        return true;
    }
    if s.level < 1.0 || covered {
        return false;
    }
    s.stamp != now || (s.flagged && s.segment != segment)
}

/// Mark the cells whose smallest refused model fits in the room the budget leaves now
/// ([`mark_refused`]): called before spawning and again after the frame's replacements,
/// so room freed late in a frame is seen at once, with no baseline to go stale.
fn retry_refused(st: &mut DistantState) {
    let room = FULL_TRIANGLE_BUDGET.saturating_sub(st.budget().total());
    mark_refused(st, room);
}

/// Mark the drawn cells with a model refused for the budget that would fit in `room`
/// triangles: judged again (capacity came back).
fn mark_refused(st: &mut DistantState, room: u64) {
    for s in st.cells.values_mut() {
        if s.refused_min.is_some_and(|m| m <= room) {
            s.dirty = true;
        }
    }
}

/// Mark the drawn cells and the cells being spawned that hold one of `changed` (their
/// override came or went): judged again.
fn mark_overridden(st: &mut DistantState, changed: &HashSet<u32>) {
    for s in st.cells.values_mut() {
        if changed.iter().any(|r| s.plan.contains_key(r)) {
            s.dirty = true;
        }
    }
    for sp in st.spawning.values_mut() {
        if sp.list.entries.iter().any(|e| changed.contains(&e.p.form_id)) {
            sp.dirty = true;
        }
    }
}

/// Take the shapes of the references in `came` (an override came) out of every distant
/// cell now: drawn cells, the cells they replace and cells being spawned. A reference's
/// entities are the children of its cell carrying its `RefVisual` (`world::spawn_model`
/// puts its shapes, rig root and holders there); they are despawned, the reference is
/// recorded as left out and its triangles leave the budget.
fn hide_overridden(commands: &mut Commands, st: &mut DistantState, came: &HashSet<u32>, children: &Query<&Children>, refs: &Query<&nvf_play::RefVisual>) {
    if came.is_empty() {
        return;
    }
    let mut take = |cell: Entity| {
        for &k in children.get(cell).map(|c| &c[..]).unwrap_or(&[]) {
            if refs.get(k).is_ok_and(|r| came.contains(&r.0)) {
                commands.entity(k).try_despawn();
            }
        }
    };
    for s in st.cells.values_mut() {
        if !overridden_here(s, came) {
            continue;
        }
        take(s.entity);
        if let Some(o) = s.old.as_mut() {
            take(o.entity);
            o.full_of.retain(|r, _| !came.contains(r));
            o.drawn.retain(|r| !came.contains(r));
        }
        for r in came {
            if let Some(c) = s.plan.get_mut(r).filter(|c| c.drawn()) {
                *c = Choice::Skipped;
            }
            s.full_of.remove(r);
        }
    }
    for sp in st.spawning.values_mut() {
        if !came.iter().any(|r| sp.drawn.get(r).is_some_and(|c| c.drawn())) {
            continue;
        }
        take(sp.entity);
        for r in came {
            if let Some(c) = sp.drawn.get_mut(r).filter(|c| c.drawn()) {
                *c = Choice::Skipped;
            }
            sp.full_of.remove(r);
        }
    }
}

/// Whether a drawn cell or the cell it replaces draws one of `came` (from its LOD or its
/// full model).
fn overridden_here(s: &CellState, came: &HashSet<u32>) -> bool {
    came.iter().any(|r| s.plan.get(r).is_some_and(|c| c.drawn()) || s.old.as_ref().is_some_and(|o| o.drawn.contains(r)))
}

/// What a distant reference's spawn reads.
struct SpawnCtx<'a> {
    world: &'a WorldData,
    streamer: &'a Streamer,
    simulated: bool,
}

/// Spawn a distant cell's references until all are in or the deadline has passed; those
/// whose model was released since go to `later`, tried again once the rest are in. Never
/// prepares a model on this thread.
#[allow(clippy::too_many_arguments)]
fn spawn_refs(commands: &mut Commands, ctx: &SpawnCtx, st: &mut DistantState, sp: &mut CellSpawn, budget: &mut Budget, deadline: Instant, models: &mut ModelCtx, emittance: &mut EmittanceState) {
    // A rebuilt cell's predecessor goes once it rests ([`admits`]): one replacement at a
    // time counts without its predecessor's share.
    let predecessor = if sp.replaces && st.credit.is_none_or(|c| c == sp.grid) {
        st.credit = Some(sp.grid);
        st.cells.get(&sp.grid).map_or(0, CellState::full_tris)
    } else {
        0
    };
    while sp.next < sp.plan.len() && Instant::now() < deadline {
        let (i, ch) = sp.plan[sp.next];
        sp.next += 1;
        if !spawn_one(commands, ctx, st, sp, (budget, predecessor), i, ch, models, emittance) {
            sp.later.push((i, ch));
        }
    }
    if sp.next >= sp.plan.len() && !sp.later.is_empty() {
        sp.plan = std::mem::take(&mut sp.later);
        sp.next = 0;
    }
}

/// One planned reference; false when its model is not prepared (asked of the workers).
#[allow(clippy::too_many_arguments)]
fn spawn_one(commands: &mut Commands, ctx: &SpawnCtx, st: &mut DistantState, sp: &mut CellSpawn, (budget, predecessor): (&mut Budget, u64), i: usize, ch: Choice, models: &mut ModelCtx, emittance: &mut EmittanceState) -> bool {
    let list = sp.list.clone();
    let e = &list.entries[i];
    let r = e.p.form_id;
    if !ch.drawn() {
        sp.drawn.insert(r, ch);
        return true;
    }
    // An override that came since the plan, or not shown for its enable state (the game's
    // simulation shows and hides the rest): recorded as left out, as the plan would.
    if ctx.streamer.model_overrides.contains_key(&r) || !full_detail::drawn_for_enable_state(e.enabled, e.p.deleted(), ctx.simulated) {
        sp.drawn.insert(r, Choice::Skipped);
        return true;
    }
    let Some((path, alt)) = model_of(e, ch) else { return true };
    if !models.models_ready([(path, alt)]) {
        return false;
    }
    let mut tris = 0;
    let base = match ch {
        Choice::Full => {
            tris = models.model_swapped(path, false, alt).map_or(0, |m| m.triangles as u64);
            st.tris_of.insert(e.full.clone(), tris);
            if !admits(*budget, predecessor, tris, FULL_TRIANGLE_BUDGET) {
                // Refused for now, not too small: judged again when capacity comes back.
                sp.drawn.insert(r, Choice::Budget);
                sp.refused_min = Some(sp.refused_min.map_or(tris, |m| m.min(tris)));
                return true;
            }
            (*e.base).clone()
        }
        _ => BaseObject { world_model: Some(path.to_string()), world_alt_textures: Vec::new(), ..(*e.base).clone() },
    };
    let rt = Affine3A::from_translation(-sp.origin) * world::ref_affine(&e.p);
    let drawn = world::spawn_model(commands, sp.entity, ctx.world, &e.p, r, &base, rt, models, emittance, &ctx.streamer.hide, &mut st.models, SpawnMode::Scenery);
    // The cell's reservation (read back by [`DistantState::budget`]) and this frame's copy.
    if drawn && ch == Choice::Full {
        sp.full_of.insert(r, tris);
        budget.reserved += tris;
    }
    sp.drawn.insert(r, if drawn { ch } else { Choice::Small });
    true
}

/// A distant cell is all in: shown (its meshes get their range in [`fade_distant`]); a
/// rebuilt one takes over from the cell it replaces by crossfading when fades are on and
/// `crossfade` ([`crossfade_ok`]), else at once.
fn finish(commands: &mut Commands, st: &mut DistantState, c: Grid, sp: CellSpawn, fading: bool, crossfade: bool) {
    let mut old = None;
    let mut level = if fading { 0.0 } else { 1.0 };
    if let Some(prev) = st.cells.remove(&c) {
        // Going already: it stays as it is and the rebuild is dropped.
        if prev.leaving {
            st.retire(commands, sp.entity);
            st.cells.insert(c, prev);
            return;
        }
        if let Some(o) = prev.old {
            st.retire(commands, o.entity);
        }
        if fading && crossfade {
            let drawn = prev.plan.iter().filter(|(_, c)| c.drawn()).map(|(r, _)| *r).collect();
            old = Some(Old { entity: prev.entity, full_of: prev.full_of, drawn });
        } else {
            st.retire(commands, prev.entity);
            level = 1.0;
        }
    }
    commands.entity(sp.entity).insert(Visibility::Inherited);
    for ch in sp.drawn.values() {
        match ch {
            Choice::Lod => st.stats.lod += 1,
            Choice::Full => st.stats.full += 1,
            Choice::Held => st.stats.held += 1,
            Choice::Unknown => st.stats.unknown += 1,
            Choice::Small => st.stats.small += 1,
            Choice::Budget => st.stats.refused += 1,
            Choice::Skipped => {}
        }
    }
    st.stats.small += sp.list.small;
    st.stats.cells += 1;
    let flagged = sp.list.entries.iter().any(|e| e.flagged);
    let state = CellState {
        entity: sp.entity,
        level,
        leaving: false,
        under: false,
        plan: sp.drawn,
        stamp: sp.stamp,
        segment: sp.segment,
        flagged,
        dirty: sp.dirty,
        full_of: sp.full_of,
        refused_min: sp.refused_min,
        old,
        applied: None,
        old_applied: None,
        extent: None,
    };
    st.cells.insert(c, state);
    st.cells_rev += 1;
}

/// A mesh's position in game space: its transforms up to its distant cell composed onto
/// the cell's origin (the frame a mesh is spawned its global transform is not yet there).
fn game_position(e: Entity, cell: Entity, origin: Vec3, transforms: &Query<&Transform>, parents: &Query<&ChildOf>) -> Vec3 {
    let mut m = Affine3A::IDENTITY;
    let mut at = e;
    while at != cell {
        let Ok(t) = transforms.get(at) else { break };
        m = t.compute_affine() * m;
        let Ok(p) = parents.get(at) else { break };
        at = p.parent();
    }
    origin + Vec3::from(m.translation)
}

/// Give each distant cell's meshes their range ([`distant_range`]: the complement of a
/// loaded or ring cell over it, its own fade composed with the reach's per mesh
/// ([`band_range`]), or the distance fade at rest) and no sun shadow; the cell a rebuilt
/// one replaces draws the complement of the new one's step. Cells fading out after Off
/// keep the last reach's fade. Runs after [`stream`], before `fade::fade_cells`. A frame
/// with nothing to give returns at once: the stream quiet, nothing given last pass, and
/// the eye where it was (or no range depending on it); a pass walks only the cells
/// [`regive`] names.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn fade_distant(
    mut commands: Commands,
    streamer: Res<Streamer>,
    ring: Res<FullDetailRing>,
    mut state: ResMut<DistantState>,
    (children, parents, transforms): (Query<&Children>, Query<&ChildOf>, Query<&Transform>),
    meshes: Query<(), Fading>,
    casters: Query<Has<NotShadowCaster>, With<Mesh3d>>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    moved: Query<&ChildOf, (Changed<Transform>, With<nvf_play::RefVisual>)>,
) {
    if state.cells.is_empty() {
        return;
    }
    // A reference moved (by a script, physics or the console: `nvf_play::moved`) under a
    // still eye changes no input of [`regive`]: its cell's ranges and box are given again.
    let moved_in: HashSet<Entity> = moved.iter().map(ChildOf::parent).collect();
    if !moved_in.is_empty() {
        let st = &mut *state;
        for s in st.cells.values_mut().filter(|s| moved_in.contains(&s.entity)) {
            s.applied = None;
            st.fade_dirty = true;
        }
    }
    let eye = camera.single().map(|g| crate::world::bevy_to_game(g.translation())).ok();
    if !state.fade_dirty && !(state.watch && state.eye_at != eye) {
        return;
    }
    let started = Instant::now();
    let st = &mut *state;
    st.eye_at = eye;
    let reach = if st.reach > 0 { st.reach } else { st.fade_reach.max(1) };
    let mut changed = false;
    let mut watch = false;
    for (c, s) in &mut st.cells {
        let cover = (streamer.loaded.contains_key(c) || ring.cells.contains_key(c)).then(|| scenery_level(streamer.cell_level(*c), &ring.cells, *c));
        let want = distant_range(s.level, cover);
        // The eye a distance-dependent range is measured from ([`band_range`], [`under_range`]).
        let dependent = matches!(want, DistantRange::Own(_) | DistantRange::Under(_));
        watch |= dependent;
        let seen = eye.filter(|_| dependent);
        let zone = seen.map_or(0, |e| cell_zone(s.extent, e, reach));
        if regive(s.applied, want, reach, zone, seen) {
            changed = true;
            let first = s.applied.is_none();
            let origin = cell_origin(*c);
            let mut extent: Option<(Vec3, Vec3)> = None;
            for e in children.iter_descendants(s.entity) {
                if meshes.contains(e) {
                    let d = match seen {
                        Some(eye) => {
                            let p = game_position(e, s.entity, origin, &transforms, &parents);
                            extent = Some(extent.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
                            p.distance(eye)
                        }
                        None => 0.0,
                    };
                    commands.entity(e).insert(want.range(d, reach));
                }
                if first && matches!(casters.get(e), Ok(false)) {
                    commands.entity(e).insert(NotShadowCaster);
                }
            }
            // The zone from the meshes' positions just composed.
            if seen.is_some() {
                s.extent = extent;
            }
            let zone = seen.map_or(zone, |e| cell_zone(s.extent, e, reach));
            s.applied = Some(Applied { range: want, reach, zone, eye: seen });
        }
        if let Some(old) = s.old.as_ref().map(|o| o.entity) {
            let k = want.own_step();
            if s.old_applied != Some(k) {
                changed = true;
                s.old_applied = Some(k);
                let range = fade::complement(k);
                for e in children.iter_descendants(old).filter(|&e| meshes.contains(e)) {
                    commands.entity(e).insert(range.clone());
                }
            }
        }
    }
    st.watch = watch;
    st.fade_dirty = changed || !st.quiet;
    st.stats.ms += started.elapsed().as_secs_f64() * 1000.0;
}

/// The exit summary (module notes), only when the distant objects were on at some time.
pub fn report(mut exit: MessageReader<AppExit>, state: Res<DistantState>) {
    if exit.read().last().is_none() || state.used.is_none() {
        return;
    }
    let s = &state.stats;
    info!(
        "distant objects summary: {} cells, {} drawn ({} lod, {} full), {} held by blocks, {} block unknown, {} too small, {} past the triangle budget, {:.0} ms on the main thread",
        s.cells,
        s.lod + s.full,
        s.lod,
        s.full,
        s.held,
        s.unknown,
        s.small,
        s.refused,
        s.ms
    );
}

pub struct DistantObjectsPlugin;

impl Plugin for DistantObjectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DistantState>()
            .add_systems(Update, (stream.run_if(world::world_not_held), fade_distant).chain().after(full_detail::fade_ring).before(fade::fade_cells))
            .add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const J: Judge = Judge { flagged: false, has_lod: false, radius: 400.0 };

    fn no_skip(_: &Entry) -> bool {
        false
    }

    /// `list_cell` takes the renderer's rule (`world::drawn_with_cell`: no imposter, no
    /// primitive) and adds only that a deleted reference is not listed. Synthetic
    /// references; no game data.
    #[test]
    fn list_cell_takes_drawn_with_cell() {
        for imposter in [false, true] {
            for primitive in [false, true] {
                for deleted in [false, true] {
                    let mut p = crate::world::tests::flagged_placement(imposter, primitive);
                    if deleted {
                        p.flags |= fnv_world::placed::ref_flags::DELETED;
                    }
                    assert_eq!(listed_ref(&p), !deleted && world::drawn_with_cell(&p), "imposter {imposter}, primitive {primitive}, deleted {deleted}");
                    assert_eq!(listed_ref(&p), !deleted && !imposter && !primitive);
                }
            }
        }
    }

    /// Every branch of the rule: a flagged reference is left to a block that holds it or
    /// is not known yet and drawn like the rest when its segment is empty; a LOD model wins
    /// over the size rule; the full model only when large enough for its distance, with the
    /// hysteresis for one drawn full before; no radius, never full.
    #[test]
    fn choose_takes_every_branch() {
        let flagged = Judge { flagged: true, ..J };
        let d = 8.0 * CELL_SIZE;
        assert_eq!(choose(flagged, None, d, SIZE_RATIO, false), Choice::Unknown);
        assert_eq!(choose(flagged, Some(true), d, SIZE_RATIO, false), Choice::Held);
        assert_eq!(choose(Judge { has_lod: true, ..flagged }, Some(true), d, SIZE_RATIO, false), Choice::Held, "held whatever its models");
        assert_eq!(choose(Judge { has_lod: true, ..flagged }, Some(false), d, SIZE_RATIO, false), Choice::Lod, "an empty segment: drawn");
        assert_eq!(choose(Judge { has_lod: true, ..J }, None, d, SIZE_RATIO, false), Choice::Lod, "unflagged: the block never holds it");
        assert_eq!(choose(Judge { has_lod: true, radius: 1.0, ..J }, Some(true), d, SIZE_RATIO, false), Choice::Lod, "a LOD model at any size");
        // 400 units at 8 cells (32,768): 0.0122 of the distance.
        assert_eq!(choose(J, Some(true), d, SIZE_RATIO, false), Choice::Full);
        assert_eq!(choose(J, Some(true), 12.0 * CELL_SIZE, SIZE_RATIO, false), Choice::Small, "0.0081 at 12 cells");
        assert_eq!(choose(J, Some(true), 12.0 * CELL_SIZE, SIZE_RATIO, true), Choice::Full, "drawn full before: within the hysteresis");
        assert_eq!(choose(J, Some(true), 14.0 * CELL_SIZE, SIZE_RATIO, true), Choice::Small, "0.0070, under 0.8 of the threshold");
        assert_eq!(choose(Judge { radius: 0.0, ..J }, None, 1.0, SIZE_RATIO, true), Choice::Small, "no bounds");
        assert_eq!(choose(J, None, d, 0.02, false), Choice::Small, "a raised ratio");
        assert!(Choice::Lod.drawn() && Choice::Full.drawn());
        assert!(![Choice::Held, Choice::Unknown, Choice::Small].iter().any(|c| c.drawn()));
    }

    /// The hysteresis band: a model enters at the threshold and leaves only under 0.8 of
    /// it, so a centre moving to and fro does not switch it.
    #[test]
    fn size_rule_has_hysteresis() {
        let d = 10_000.0;
        assert!(size_passes(100.0, d, 0.01, false));
        assert!(!size_passes(99.0, d, 0.01, false));
        assert!(size_passes(99.0, d, 0.01, true));
        assert!(size_passes(80.0, d, 0.01, true));
        assert!(!size_passes(79.0, d, 0.01, true));
        assert!(!size_passes(0.0, 0.0, 0.01, true), "no radius");
    }

    /// LOD models beside the full model: `_lod.nif` first, else `_lod_0.nif`, else none; a
    /// model that is itself a LOD model has none.
    #[test]
    fn lod_models_sit_beside_the_full_model() {
        let have = ["meshes\\rocks\\rock01_lod.nif", "meshes\\rocks\\rock01_lod_0.nif", "meshes\\signs\\sign_lod_0.nif"];
        let exists = |p: &str| have.contains(&p);
        assert_eq!(lod_model("meshes\\rocks\\rock01.nif", exists).as_deref(), Some("meshes\\rocks\\rock01_lod.nif"));
        assert_eq!(lod_model("meshes\\signs\\sign.nif", exists).as_deref(), Some("meshes\\signs\\sign_lod_0.nif"), "a pack's own naming");
        assert_eq!(lod_model("meshes\\shacks\\shack.nif", exists), None);
        assert_eq!(lod_model("meshes\\rocks\\rock01_lod.nif", exists), None, "already a LOD model");
        assert_eq!(lod_model("meshes\\x.kf", exists), None);
    }

    /// The budget: within it every ring keeps the base ratio; past it the farthest ring is
    /// raised first, up to the most, then the next; the sum then fits, or every ring with
    /// candidates is at the most.
    #[test]
    fn the_budget_raises_the_ratio_of_the_farthest_cells_first() {
        let c = |steps, size, triangles| Candidate { steps, size, triangles };
        let cands = [c(3, 0.05, 1000), c(3, 0.012, 1000), c(8, 0.05, 1000), c(8, 0.011, 1000), c(8, 0.02, 1000)];
        let (r, raised) = fit_budget(&cands, 8, 0.01, 10_000);
        assert!(!raised && r.len() == 9 && r.iter().all(|&q| q == 0.01), "within the budget");
        // 5,000 triangles against 4,000: ring 8 goes to 0.015, which leaves out its 0.011.
        let (r, raised) = fit_budget(&cands, 8, 0.01, 4000);
        assert!(raised);
        assert!((r[8] - 0.015).abs() < 1e-6, "{r:?}");
        assert_eq!(r[3], 0.01, "the nearer ring untouched");
        // 3,000: ring 8 rises past 0.02 (0.0225), leaving its 0.05 only; 3,000 fits.
        let (r, _) = fit_budget(&cands, 8, 0.01, 3000);
        assert!((r[8] - 0.0225).abs() < 1e-6 && r[3] == 0.01, "{r:?}");
        // 2,000: ring 8 rises past its 0.05 (0.0506) and draws no full model.
        let (r, _) = fit_budget(&cands, 8, 0.01, 2000);
        assert!(r[8] > 0.05 && r[3] == 0.01, "{r:?}");
        // 1,500: ring 8 draws none, so ring 3 is raised next, past its 0.012.
        let (r, _) = fit_budget(&cands, 8, 0.01, 1500);
        assert!(r[8] > 0.05 && r[3] > 0.012 && r[3] < 0.05, "{r:?}");
        // 0: both rings raised until they draw none; rings without candidates untouched.
        let (r, raised) = fit_budget(&cands, 8, 0.01, 0);
        assert!(raised && r[3] > 0.05 && r[8] > 0.05 && r[5] == 0.01, "{r:?}");
        // A model too large for any ratio: its ring at the most, and the loop ends.
        let (r, raised) = fit_budget(&[c(4, 0.5, 1000)], 8, 0.01, 0);
        assert!(raised && r[4] == RATIO_MAX && r[8] == 0.01, "{r:?}");
        assert_eq!(ratio_at(&r, 4), RATIO_MAX);
        assert_eq!(ratio_at(&r, 20), SIZE_RATIO, "past the table");
        assert_eq!(ratio_at(&r, -1), SIZE_RATIO);
    }

    /// The cells: one step beyond the inner radius out to the reach, none whose nearest
    /// point is past the reach's distance (the square's corners), nearest first.
    #[test]
    fn distant_cells_are_beyond_the_inner_radius_nearest_first() {
        let centre = (-17, 2);
        let cells = distant_cells(centre, 2, 8);
        assert!(cells.iter().all(|&c| (3..=8).contains(&steps(c, centre))));
        assert!(cells.contains(&(centre.0 + 8, centre.1)), "a side's middle at the reach");
        assert!(!cells.contains(&(centre.0 + 8, centre.1 + 8)), "a corner past the reach's distance");
        assert!(!cells.contains(&(centre.0 + 2, centre.1)), "a loaded cell");
        let keys: Vec<i32> = cells.iter().map(|&c| order_key(c, centre)).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "nearest first");
        assert_eq!(steps(cells[0], centre), 3);
        let p = centre_point(centre);
        assert!(cells.iter().all(|&c| crate::lod::square_distance(p, c, 1) < 8.0 * CELL_SIZE));
        // Beyond the 9 x 9 ring: from 5 steps.
        assert!(distant_cells(centre, 4, 8).iter().all(|&c| steps(c, centre) >= 5));
        assert!(distant_cells(centre, 4, 12).len() > distant_cells(centre, 4, 8).len());
        assert!(distant_cells(centre, 8, 8).is_empty());
        assert_eq!(nearest(2), 2.5 * CELL_SIZE);
    }

    /// CPU mirror of Bevy's visibility-range dither (as full_detail.rs's tests): whether a
    /// pixel with dither `threshold` is drawn by a mesh with range `r` at distance `d`.
    fn kept(r: &VisibilityRange, d: f32, threshold: i32) -> bool {
        // Culled on the CPU outside the range (`VisibilityRange::is_visible_at_all`).
        if !r.is_visible_at_all(d) {
            return false;
        }
        let (x, y, z, w) = (r.start_margin.start, r.start_margin.end, r.end_margin.start, r.end_margin.end);
        let (offset, a, b) = if d >= z { (0, z, w) } else { (-16, x, y) };
        let level = offset + (((d - a) / (b - a) * 16.0).round() as i32).clamp(0, 16);
        if level == 0 {
            return true;
        }
        if level <= -16 || level >= 16 {
            return false;
        }
        !((level >= 0 && level + threshold >= 16) || (level < 0 && 1 + level + threshold <= 0))
    }

    /// Pixel-once: a distant cell under a loaded or ring cell (drawn at any step), at any
    /// level of its own, fading in or out, together with that cell draws each pixel of a
    /// reference both draw once; with a loaded cell and a ring cell under it as well (the
    /// ring drawing the loaded cell's complement); a rebuilt cell and the one it replaces
    /// likewise; alone at rest it draws every pixel inside the reach's distance fade.
    #[test]
    fn distant_loaded_and_ring_cover_each_pixel_once() {
        let d = 20_000.0;
        let s = fade::STEPS as i32;
        for kd in 0..=s {
            for (leaving, under) in [(false, false), (false, true), (true, false), (true, true)] {
                for kc in 0..=s {
                    let cover = kc as f32 / s as f32;
                    let Some(level) = hand_off(kd as f32 / s as f32, leaving, Some(kc == s), under) else { continue };
                    let mine = distant_range(level, Some(cover)).range(d, REACH_NEAR);
                    let theirs = fade::drawn(kc as u8);
                    for t in 0..16 {
                        let n = [&mine, &theirs].iter().filter(|r| kept(r, d, t)).count();
                        assert_eq!(n, 1, "distant step {kd} leaving {leaving}, cover step {kc}: pixel {t} drawn {n} times");
                    }
                    // A loaded cell at step kc with a ring cell under it (ring at rest, its
                    // complement): the cover level is the ring's 1, the distant cell culled;
                    // also when the ring cell's trees fell back to the billboards
                    // (`trees_kept`): its statics are still drawn (the tree level would not
                    // count it).
                    let ring = full_detail::ring_range(1.0, Some(cover)).range();
                    for state in [ring_state(1.0), full_detail::RingCellState::for_test_trees_kept(1.0)] {
                        let level_here = scenery_level(cover, &HashMap::from([((3, 0), state)]), (3, 0));
                        let mine = distant_range(level, Some(level_here)).range(d, REACH_NEAR);
                        for t in 0..16 {
                            let n = [&mine, &theirs, &ring].iter().filter(|r| kept(r, d, t)).count();
                            assert_eq!(n, 1, "with a ring cell under the loaded one, cover step {kc}: pixel {t} drawn {n} times");
                        }
                    }
                }
            }
        }
        // A rebuilt cell fading in over the one it replaces.
        for k in 0..=s {
            let new = distant_range(k as f32 / s as f32, None);
            let old = fade::complement(new.own_step());
            for t in 0..16 {
                let n = [&new.range(d, REACH_FAR), &old].iter().filter(|r| kept(r, d, t)).count();
                assert_eq!(n, 1, "rebuild step {k}: pixel {t} drawn {n} times");
            }
        }
        // At rest and alone: the distance fade.
        assert_eq!(distant_range(1.0, None), DistantRange::Edge);
        let edge = DistantRange::Edge.range(0.0, REACH_NEAR);
        assert!(fade::at_rest(&edge), "a fully drawn range (the sliders' test)");
        assert!((0..16).all(|t| kept(&edge, 6.5 * CELL_SIZE, t)), "inside reach - 1: every pixel");
        assert!((0..16).all(|t| !kept(&edge, 8.1 * CELL_SIZE, t)), "past the reach: none");
        let half = (0..16).filter(|&t| kept(&edge, 7.5 * CELL_SIZE, t)).count();
        assert!(half > 0 && half < 16, "between: dithering out ({half})");
        assert!((edge_range(REACH_FAR).end_margin.end - 12.0 * CELL_SIZE).abs() < 1.0);
        // The hand-off rule is the ring's: held at rest under a cover, dropped when leaving.
        assert_eq!(hand_off(0.3, false, Some(false), false), Some(1.0));
        assert_eq!(hand_off(0.3, true, Some(false), false), None);
        assert_eq!(hand_off(1.0, true, Some(false), true), Some(1.0), "a leaving cell already under a cover keeps its complement");
        assert_eq!(distant_range(0.5, None), DistantRange::Own(8));
        assert_eq!(distant_range(0.5, Some(0.25)), DistantRange::Under(4));
    }

    /// The reach's fade holds while a cell fades in or out: at every temporal step nothing
    /// beyond the reach shows, inside `reach - 1` the step alone decides (and the old cell
    /// of a rebuild draws exactly the rest there), and in the band between the distance's
    /// dither decides. Rebuilds crossfade only short of that band.
    #[test]
    fn the_reach_fade_holds_during_fades() {
        for reach in [REACH_NEAR, REACH_FAR] {
            for k in 0..fade::STEPS {
                let beyond = reach as f32 * CELL_SIZE + 100.0;
                let far = DistantRange::Own(k).range(beyond, reach);
                assert!((0..16).all(|t| !kept(&far, beyond, t)), "reach {reach}, step {k}: something beyond the reach");
                let inside = (reach - 1) as f32 * CELL_SIZE - 500.0;
                let own = DistantRange::Own(k).range(inside, reach);
                let shown = (0..16).filter(|&t| kept(&own, inside, t)).count();
                let plain = (0..16).filter(|&t| kept(&fade::drawn(k), inside, t)).count();
                assert_eq!(shown, plain, "reach {reach}, step {k}: inside, the temporal step alone");
                let old = fade::complement(k);
                for t in 0..16 {
                    let n = [&own, &old].iter().filter(|r| kept(r, inside, t)).count();
                    assert_eq!(n, 1, "reach {reach}, step {k}: a rebuild's pixel {t} drawn {n} times");
                }
            }
        }
        // Rebuilds: a cell whose far corner is within reach - 2 of the middle cell's centre
        // crossfades, one nearer the edge swaps at once.
        assert!(crossfade_ok((4, 0), (0, 0), REACH_NEAR), "far corner 4.5 cells out");
        assert!(!crossfade_ok((6, 0), (0, 0), REACH_NEAR), "far corner 6.5 cells out");
        assert!(crossfade_ok((9, 0), (0, 0), REACH_FAR));
        assert!(!crossfade_ok((10, 0), (0, 0), REACH_FAR));
        assert!(!crossfade_ok((5, 5), (0, 0), REACH_NEAR), "a diagonal");
    }

    /// A resting ring cell whose trees fell back to the billboards still covers its square's
    /// statics: the distant cell under it is culled (the ring's tree level would read the
    /// cell as absent and let the distant copy draw fully beside the ring's statics).
    #[test]
    fn a_ring_cell_with_billboard_trees_still_covers_the_statics() {
        let ring = HashMap::from([((3, 0), full_detail::RingCellState::for_test_trees_kept(1.0))]);
        assert_eq!(full_detail::tree_level(0.0, Some(&ring), (3, 0)), 0.0, "the tree level ignores it");
        assert_eq!(scenery_level(0.0, &ring, (3, 0)), 1.0, "the scenery level counts it");
        assert_eq!(distant_range(1.0, Some(scenery_level(0.0, &ring, (3, 0)))), DistantRange::Under(fade::STEPS));
        assert_eq!(scenery_level(0.25, &HashMap::new(), (3, 0)), 0.25, "the loaded cell alone");
    }

    /// The temporal step and the reach's fade compose as the smaller of the two at every
    /// distance: inside the band the step alone (even step 0 draws nothing at the band's
    /// start), across the band never more than either, nothing beyond the reach, and at rest
    /// the band range meets the distance fade it hands over to.
    #[test]
    fn the_temporal_step_and_the_reach_fade_compose() {
        let shown = |r: &VisibilityRange, d: f32| (0..16).filter(|&t| kept(r, d, t)).count() as u8;
        for reach in [REACH_NEAR, REACH_FAR] {
            let (z, w) = edge_band(reach);
            for d in [z - 200.0, z + 1.0, (z + w) / 2.0, w - 1.0, w + 10.0] {
                for k in 0..=fade::STEPS {
                    let r = band_range(k, d, reach);
                    let edge = edge_steps(d, reach);
                    assert_eq!(shown(&r, d), k.min(edge), "reach {reach}, step {k}, {d}: the smaller of the two");
                }
                assert_eq!(shown(&band_range(fade::STEPS, d, reach), d), shown(&edge_range(reach), d), "reach {reach}, {d}: at rest it meets the distance fade");
            }
            assert_eq!(shown(&band_range(0, z + 1.0, reach), z + 1.0), 0, "step 0 at the band's start: nothing");
            assert_eq!(edge_steps(z - 1.0, reach), fade::STEPS);
            assert_eq!(edge_steps(w + 1.0, reach), 0);
            assert_eq!(edge_steps((z + w) / 2.0, reach), fade::STEPS / 2);
        }
    }

    /// Off fades the cells out with the last reach's distance fade (not a reach of 1, which
    /// culled them on the switch frame); a predecessor that may no longer crossfade after a
    /// shrink (cell (6, 6) at 12 cells, past 8 cell widths diagonally at 8) goes at once and
    /// its new cell takes over.
    #[test]
    fn off_and_a_shrink_mid_fade() {
        assert!(crossfade_ok((6, 6), (0, 0), REACH_FAR), "a 12-cell crossfade");
        assert!(!crossfade_ok((6, 6), (0, 0), REACH_NEAR), "not after a shrink to 8");
        assert!(predecessor_goes(0.4, false, false, crossfade_ok((6, 6), (0, 0), REACH_NEAR)), "the predecessor goes at once");
        assert!(!predecessor_goes(0.4, false, false, true), "otherwise it crossfades on");
        assert!(predecessor_goes(1.0, false, false, true) && predecessor_goes(0.4, true, false, true) && predecessor_goes(0.4, false, true, true));
        // Off: the fade reach stays the last one on.
        let mut st = DistantState { reach: REACH_FAR, fade_reach: REACH_FAR, ..default() };
        st.reach = 0;
        let fade_reach = if st.reach > 0 { st.reach } else { st.fade_reach.max(1) };
        assert_eq!(fade_reach, REACH_FAR);
        let mid = 6.0 * CELL_SIZE;
        assert_eq!((0..16).filter(|&t| kept(&DistantRange::Own(8).range(mid, fade_reach), mid, t)).count(), 8, "half drawn mid-fade, as before the switch");
    }

    /// Admission outlives no shortage: a model refused for the budget is recorded apart
    /// from one too small, and its cell is marked once capacity for it comes back; one
    /// rebuilt cell at a time counts without its predecessor, so the total passes the cap by
    /// at most that predecessor's share.
    #[test]
    fn refused_models_come_back_with_capacity() {
        assert!(!Choice::Budget.drawn() && Choice::Budget != Choice::Small);
        let mut st = DistantState::default();
        let mut s = drawn_cell(0, None);
        s.refused_min = Some(3000);
        st.cells.insert((9, 0), s);
        mark_refused(&mut st, 2000);
        assert!(!st.cells[&(9, 0)].dirty, "no room for it yet");
        mark_refused(&mut st, 3000);
        assert!(st.cells[&(9, 0)].dirty, "room again: judged again");
        // The credit: held by a replacement while it spawns or crossfades, then released.
        st.credit = Some((9, 0));
        st.release_credit();
        assert_eq!(st.credit, None, "neither spawning nor crossfading: released");
        st.spawning.insert((9, 0), a_spawn((9, 0), true, 0));
        st.credit = Some((9, 0));
        st.release_credit();
        assert_eq!(st.credit, Some((9, 0)), "its replacement spawns");
    }

    /// The re-judge scan goes on where it stopped, in a stable order, so every cell is
    /// reached however short the frames; a fit for another centre or reach is not taken.
    #[test]
    fn the_scan_goes_round() {
        assert_eq!(scan_next(0, 64, 100), 64);
        assert_eq!(scan_next(64, 64, 100), 28, "wraps round");
        assert_eq!(scan_next(5, 0, 0), 0);
        let mut st = DistantState::default();
        for x in 0..10 {
            st.cells.insert((x, 0), drawn_cell(0, None));
        }
        st.cells_rev = 1;
        let blocks = BlockContents::default();
        let now = Stamp { centre: (5, 5), ratios: 0 };
        queue_rebuilds(&mut st, now, &blocks, &|_| None, Instant::now() + Duration::from_secs(1));
        assert_eq!(st.rebuild.len(), 10, "every stale cell");
        assert_eq!(st.scan, (0..10).map(|x| (x, 0)).collect::<Vec<_>>(), "a stable order");
    }

    /// A settled frame compares its inputs: the same ones leave a quiet state quiet, any
    /// change (the centre, the ring's cells moving, a block, an override) does not.
    #[test]
    fn a_settled_frame_compares_its_inputs() {
        let a = Settle { centre: Some((0, 0)), inner: 2, reach: 8, lod: true, ..default() };
        assert_eq!(a, Settle { ..a });
        assert_ne!(a, Settle { centre: Some((1, 0)), ..a });
        assert_ne!(a, Settle { ring_moving: true, ..a });
        assert_ne!(a, Settle { blocks: 1, ..a });
        assert_ne!(a, Settle { overrides: 1, ..a });
    }

    /// Round 3: at and beyond the reach a fading mesh draws nothing at any step, also in
    /// the interval the earlier test skipped (the reach itself and just past it), and the
    /// last stretch of the band draws nothing either; the resting range likewise.
    #[test]
    fn nothing_at_or_beyond_the_reach() {
        for reach in [REACH_NEAR, REACH_FAR] {
            let (z, w) = edge_band(reach);
            for d in [w - 100.0, w - 1.0, w - 0.01, w, w + 0.01, w + 0.5, w + 1.0, w + 1000.0] {
                for k in 0..=fade::STEPS {
                    let r = band_range(k, d, reach);
                    assert!((0..16).all(|t| !kept(&r, d, t)), "reach {reach}, step {k}, {d}: drawn at the reach");
                }
                assert!((0..16).all(|t| !kept(&edge_range(reach), d, t)), "reach {reach}, {d}: the resting range");
            }
            assert!(band_range(fade::STEPS, z - 1.0, reach).is_visible_at_all(z - 1.0) && !band_range(0, w, reach).is_visible_at_all(w));
        }
    }

    /// Closing review: a cell the scan passed while it was skipped (fading), whose block
    /// changed meanwhile and which then comes to rest, keeps the layer awake until the scan
    /// reaches it again, and is due once it is at rest.
    #[test]
    fn a_cell_skipped_while_its_block_changed_keeps_the_layer_awake() {
        let now = Stamp::default();
        let mut s = drawn_cell(0, None);
        s.flagged = true;
        s.level = 0.5;
        assert!(!rejudge_due(&s, now, None, false, false), "fading: the scan passes it");
        // Its block arrives empty while it is skipped; then it comes to rest.
        assert!(!rejudge_due(&s, now, Some(false), false, false), "still fading");
        s.level = 1.0;
        assert!(!cell_settled(&s, now, Some(false)), "at rest with an old block answer: not settled");
        assert!(rejudge_due(&s, now, Some(false), false, false), "due once the scan comes round");
        s.segment = Some(false);
        assert!(cell_settled(&s, now, Some(false)), "judged with the answer it has now");
        s.flagged = false;
        s.segment = None;
        assert!(cell_settled(&s, now, Some(true)), "no flagged reference: the block does not matter");
    }

    /// Closing review: a mesh under a fading cover is cut at the reach too, and inside the
    /// band draws no more than the reach's fade; short of the band it is the plain
    /// complement the hand-off needs.
    #[test]
    fn the_covered_path_keeps_the_reach() {
        for reach in [REACH_NEAR, REACH_FAR] {
            let (z, w) = edge_band(reach);
            for k in 0..=fade::STEPS {
                for d in [w, w + 0.01, w + 1.0, w + 1000.0] {
                    assert!((0..16).all(|t| !kept(&DistantRange::Under(k).range(d, reach), d, t)), "reach {reach}, step {k}, {d}: covered, drawn at the reach");
                }
                let mid = (z + w) / 2.0;
                let shown = (0..16).filter(|&t| kept(&DistantRange::Under(k).range(mid, reach), mid, t)).count() as u8;
                assert_eq!(shown, edge_steps(mid, reach).min(fade::STEPS - k), "reach {reach}, step {k}: in the band, the smaller");
                let near = z - 500.0;
                assert!(DistantRange::Under(k).range(near, reach) == fade::complement(k), "short of the band: the complement");
            }
        }
    }

    /// Review of the closing fixes: under a cover the distant mesh keeps the complementary
    /// pattern through the band: just below the band's inner edge and exactly at it the
    /// cover and the mesh draw every pixel once (the union of both masks), and inside the
    /// band they never draw one pixel twice.
    #[test]
    fn the_covered_pattern_stays_complementary_into_the_band() {
        for reach in [REACH_NEAR, REACH_FAR] {
            let (z, w) = edge_band(reach);
            for k in 0..=fade::STEPS {
                let cover = fade::drawn(k);
                for d in [z - 0.5, z] {
                    let mine = DistantRange::Under(k).range(d, reach);
                    for t in 0..16 {
                        let n = [&cover, &mine].iter().filter(|r| kept(r, d, t)).count();
                        assert_eq!(n, 1, "reach {reach}, step {k}, {d}: pixel {t} drawn {n} times");
                    }
                }
                for d in [z + 100.0, (z + w) / 2.0, w - 100.0] {
                    let mine = DistantRange::Under(k).range(d, reach);
                    assert!((0..16).all(|t| !(kept(&cover, d, t) && kept(&mine, d, t))), "reach {reach}, step {k}, {d}: a pixel drawn twice in the band");
                }
            }
        }
    }

    /// Review of the closing fixes, round 2: a cell's zone against the band is taken from
    /// its meshes' own positions (the box of them), not its grid square, and decides when
    /// its distance-dependent ranges are given again. Distances by hand, reach 8: the band
    /// runs from 28,672 to 32,768 units.
    #[test]
    fn a_cell_zone_follows_the_camera() {
        // A mesh at game x 28,336 (cell (3, 0)'s square ends at 16,384), the eye at the
        // middle of cell (0, 0): 26,288 units, short of the band; one cell west 30,384, in
        // it; two cells west 34,480, past the reach.
        let p = Vec3::new(28_336.0, 2048.0, 0.0);
        let eye = Vec3::new(2048.0, 2048.0, 0.0);
        assert_eq!(cell_zone(Some((p, p)), eye, REACH_NEAR), 0, "short of the band");
        assert_eq!(cell_zone(Some((p, p)), eye - Vec3::X * CELL_SIZE, REACH_NEAR), 1, "in the band");
        assert_eq!(cell_zone(Some((p, p)), eye - Vec3::X * 2.0 * CELL_SIZE, REACH_NEAR), 2, "past the reach");
        // A flat box from x 20,000 to 30,000: its farthest corner (27,952 east, 2,048
        // north) is 28,027 away, short of the band. With its top 20,000 units up, that
        // corner is 34,431 away and the nearest point 17,952: across the band.
        let lo = Vec3::new(20_000.0, 0.0, 0.0);
        assert_eq!(cell_zone(Some((lo, Vec3::new(30_000.0, 4096.0, 0.0))), eye, REACH_NEAR), 0, "flat: short of the band");
        assert_eq!(cell_zone(Some((lo, Vec3::new(30_000.0, 4096.0, 20_000.0))), eye, REACH_NEAR), 1, "a tall mesh reaches into the band");
        assert_eq!(cell_zone(None, eye, REACH_NEAR), 1, "no box yet: taken as across");
        // Given again for another zone; in zone 1 only once the eye has moved past EYE_STEP.
        let given = Applied { range: DistantRange::Own(15), reach: REACH_NEAR, zone: 1, eye: Some(eye) };
        let a = Some(given);
        assert!(regive(None, DistantRange::Own(15), REACH_NEAR, 1, Some(eye)), "never given");
        assert!(!regive(a, DistantRange::Own(15), REACH_NEAR, 1, Some(eye)), "a still eye: nothing");
        assert!(!regive(a, DistantRange::Own(15), REACH_NEAR, 1, Some(eye + Vec3::X * 50.0)), "50 units: nothing");
        assert!(regive(a, DistantRange::Own(15), REACH_NEAR, 1, Some(eye + Vec3::X * 100.0)), "100 units: given again");
        assert!(regive(a, DistantRange::Own(15), REACH_NEAR, 0, Some(eye)), "another zone");
        assert!(regive(a, DistantRange::Own(14), REACH_NEAR, 1, Some(eye)), "another step");
        let short = Some(Applied { zone: 0, ..given });
        assert!(!regive(short, DistantRange::Own(15), REACH_NEAR, 0, Some(eye + Vec3::X * 1000.0)), "short of the band: the eye does not matter");
    }

    /// A [`Streamer`] with nothing loaded, for the systems' tests.
    fn test_streamer() -> Streamer {
        Streamer {
            radius: 2,
            loaded: default(),
            fades: default(),
            fade_secs: 0.0,
            settled: true,
            arrival: None,
            per_frame: 1,
            spawning: default(),
            dying: default(),
            ahead: default(),
            hide: default(),
            interior: None,
            light_scale: 1.0,
            managed: default(),
            manage_actors: false,
            stats: default(),
            buffer: default(),
            centre: None,
            waiting: default(),
            model_overrides: default(),
            coop_areas: Vec::new(),
            travels: 0,
        }
    }

    /// Review of the closing fixes, round 2, through [`fade_distant`] itself: a cell fading
    /// at step 15 (level 15/16) with one mesh of cell (3, 0) offset 14,000 units east of
    /// its reference (reference at 2,048 x 2,048 in the cell, so the mesh stands at game x
    /// 28,336, well outside the grid square). From the middle of cell (0, 0) it is 26,288
    /// units away, short of the 8-cell band (28,672): drawn at 15. The eye steps one cell
    /// west: 30,384 units, 1,712 into the 4,096-unit band, 6.6875 of 16 steps, rounded 7,
    /// so the reach leaves 9 and the mesh is drawn at 9, although the cell's whole grid
    /// square is still short of the band (its farthest corner 18,545 units away; 21,070
    /// with the old 10,000-unit height allowance). Then, with the stream quiet, a still eye
    /// gives nothing again and clears the dirty flag; a move of 50 units gives nothing;
    /// one of 100 units from where the ranges were given gives them again (30,484 units:
    /// 7.078 steps, rounded 7, still 9 drawn).
    #[test]
    fn an_offset_mesh_follows_the_eye_and_a_still_eye_costs_nothing() {
        let mut app = App::new();
        let w = app.world_mut();
        w.insert_resource(test_streamer());
        w.init_resource::<FullDetailRing>();
        let cell = w.spawn(Transform::default()).id();
        let reference = w.spawn((Transform::from_translation(Vec3::new(2048.0, 2048.0, 0.0)), nvf_play::RefVisual(7), ChildOf(cell))).id();
        let mesh = w.spawn((Transform::from_translation(Vec3::new(14_000.0, 0.0, 0.0)), MeshMaterial3d::<nvf_assets::NifMaterial>(Handle::default()), ChildOf(reference))).id();
        // Game (x, y, z) is Bevy (x, z, -y).
        let at = |x: f32| GlobalTransform::from_translation(Vec3::new(x, 0.0, -2048.0));
        let cam = w.spawn((Camera3d::default(), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, at(2048.0))).id();
        let mut s = drawn_cell(0, None);
        s.entity = cell;
        s.level = 15.0 / 16.0;
        let mut st = DistantState { reach: REACH_NEAR, fade_dirty: true, quiet: true, ..default() };
        st.cells.insert((3, 0), s);
        w.insert_resource(st);
        // One system kept across the runs, so `Changed<Transform>` counts from its last run.
        let fade = w.register_system(fade_distant);
        let drawn = |app: &App, d: f32| -> Option<usize> { app.world().get::<VisibilityRange>(mesh).map(|r| (0..16).filter(|&t| kept(r, d, t)).count()) };
        let range = |app: &App| app.world().get::<VisibilityRange>(mesh).cloned();

        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app) == Some(band_range(15, 26_288.0, REACH_NEAR)));
        assert_eq!(drawn(&app, 26_288.0), Some(15), "short of the band: its own step");
        assert_eq!(app.world().resource::<DistantState>().cells[&(3, 0)].extent, Some((Vec3::new(28_336.0, 2048.0, 0.0), Vec3::new(28_336.0, 2048.0, 0.0))), "the box of the mesh's position");

        app.world_mut().entity_mut(cam).insert(at(-2048.0));
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app) == Some(band_range(15, 30_384.0, REACH_NEAR)));
        assert_eq!(drawn(&app, 30_384.0), Some(9), "in the band: the reach's 9 steps");

        // A still eye: nothing given again (the range taken off stays off), the flag clears.
        app.world_mut().entity_mut(mesh).remove::<VisibilityRange>();
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app).is_none(), "a still eye walks no mesh");
        assert!(!app.world().resource::<DistantState>().fade_dirty, "nothing given, the stream quiet: not dirty");
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app).is_none());

        app.world_mut().entity_mut(cam).insert(at(-2098.0));
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app).is_none(), "50 units: within EYE_STEP");
        app.world_mut().entity_mut(cam).insert(at(-2148.0));
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app) == Some(band_range(15, 30_484.0, REACH_NEAR)), "100 units from where it was given: given again");
        assert_eq!(drawn(&app, 30_484.0), Some(9));

        // A script moves the reference 4,000 units east under the still eye: the mesh at
        // game x 32,336 is 34,484 units away, past the reach (32,768): given again, culled.
        app.world_mut().entity_mut(mesh).remove::<VisibilityRange>();
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app).is_none(), "still nothing to give");
        app.world_mut().entity_mut(reference).insert(Transform::from_translation(Vec3::new(6048.0, 2048.0, 0.0)));
        app.world_mut().run_system(fade).unwrap();
        assert!(range(&app) == Some(band_range(15, 34_484.0, REACH_NEAR)), "the moved reference's cell given again");
        assert_eq!(drawn(&app, 34_484.0), Some(0), "past the reach: nothing");
        let p = Vec3::new(32_336.0, 2048.0, 0.0);
        assert_eq!(app.world().resource::<DistantState>().cells[&(3, 0)].extent, Some((p, p)), "the box follows the move");
    }

    /// Review of the closing fixes, round 2, driven through the frame's own steps: a
    /// flagged cell in the hysteresis step outside the wanted cells (9 steps out, reach
    /// 8), its list gone (the inner radius shrank), whose block then arrived empty. Each
    /// frame runs what [`stream`] runs, in its order: the bounded despawn of retired cells
    /// (`world::despawn_dying`), the lifecycle ([`advance_cells`]), the re-judge step
    /// ([`rejudge`]: the scan queues the cell, which has no list, so it goes) and the quiet
    /// test ([`layer_quiet`]). With fades on (a quarter a frame) it starts leaving on frame
    /// 1, fades to 0.75, 0.5, 0.25 and 0 over frames 2 to 5, is retired on frame 5 and its
    /// entity despawned on frame 6, when the layer goes quiet and stays so. With fades off
    /// it is retired on frame 1 and the layer is quiet from frame 2. Were the cell not let
    /// go, the scan would queue it on every lap and the layer would never sleep.
    #[test]
    fn an_unlisted_cell_outside_the_wanted_ones_goes() {
        use bevy::ecs::system::RunSystemOnce;
        for (fading, quiet_from) in [(true, 6), (false, 2)] {
            let mut w = World::new();
            let mut blocks = BlockContents::default();
            // A block is keyed by its level-4 square's south-west cell: (8, 0) for (9, 0).
            blocks.set_for_test((8, 0), 0);
            assert_eq!(blocks.segment((9, 0)), Some(false), "the cell's segment is known and empty");
            w.insert_resource(blocks);
            let stale_entity = w.spawn_empty().id();
            w.spawn(ChildOf(stale_entity));
            let kept_entity = w.spawn_empty().id();
            let mut st = DistantState { reach: REACH_NEAR, cells_rev: 1, ..default() };
            let mut stale = drawn_cell(0, None);
            stale.entity = stale_entity;
            stale.flagged = true;
            st.cells.insert((9, 0), stale);
            let mut kept_cell = drawn_cell(0, None);
            kept_cell.entity = kept_entity;
            st.cells.insert((5, 0), kept_cell);
            st.listed.insert((5, 0), Arc::new(CellList::default()));
            w.insert_resource(st);
            let frame = CellFrame { centre: (0, 0), inner: 2, reach: REACH_NEAR, shrank: false, fading, rate: if fading { 0.25 } else { 1.0 }, clamp_reach: REACH_NEAR };
            let now = Stamp::default();
            let mut quiet_frames = Vec::new();
            for n in 1..=12 {
                let want = vec![(5, 0)];
                let quiet = w
                    .run_system_once(move |mut commands: Commands, children: Query<&Children>, mut state: ResMut<DistantState>, blocks: Res<BlockContents>| {
                        let st = &mut *state;
                        world::despawn_dying(&mut commands, &mut st.dying, &children);
                        advance_cells(&mut commands, st, &frame, &want, &|_| None, &|_| false);
                        rejudge(&mut commands, st, now, &blocks, &|_| None, &want, frame.fading, true, Instant::now() + Duration::from_secs(5));
                        st.quiet = layer_quiet(st, false, &want, now, &blocks);
                        st.quiet
                    })
                    .unwrap();
                if quiet {
                    quiet_frames.push(n);
                }
                let st = w.resource::<DistantState>();
                if fading && n < 5 {
                    assert!(st.cells[&(9, 0)].leaving, "frame {n}: fading out");
                    assert_eq!(st.cells[&(9, 0)].level, 1.0 - 0.25 * (n - 1) as f32, "frame {n}");
                }
            }
            assert_eq!(quiet_frames, (quiet_from..=12).collect::<Vec<_>>(), "fading {fading}: quiet once the cell's entity is despawned, and from then on");
            let st = w.resource::<DistantState>();
            assert!(!st.cells.contains_key(&(9, 0)) && st.cells.contains_key(&(5, 0)) && st.dying.is_empty());
            assert!(w.get_entity(stale_entity).is_err(), "its entity despawned");
        }
    }

    /// Round 3: the quiet state needs a whole scan lap for the blocks, stamp, reach and
    /// cells now; a block arriving while a lap is mid-way starts the lap again, so the
    /// flagged cell it concerns is still looked at before the state can go quiet.
    #[test]
    fn a_late_block_mid_scan_is_still_acted_on() {
        let mut st = DistantState::default();
        for x in 0..200 {
            let mut s = drawn_cell(0, None);
            s.flagged = x == 150;
            st.cells.insert((x, 0), s);
        }
        st.cells_rev = 1;
        let mut blocks = BlockContents::default();
        let now = Stamp::default();
        // A lap cut short by the deadline: incomplete.
        queue_rebuilds(&mut st, now, &blocks, &|_| None, Instant::now());
        let key = |b: &BlockContents, st: &DistantState| ScanKey { blocks: b.rev(), now, reach: st.reach, cells: st.cells_rev };
        assert!(st.scan_seen < 200 && !scan_complete(&st, key(&blocks, &st)), "cut short at the deadline: not complete");
        assert!(st.rebuild.is_empty(), "nothing stale yet");
        // The block of cell (150, 0) arrives empty mid-lap: the lap starts again. A block
        // is keyed by its level-4 square's south-west cell: (148, 0) on the grid from (0, 0).
        blocks.set_for_test((148, 0), 0);
        assert_eq!(blocks.segment((150, 0)), Some(false), "the cell's segment is known and empty");
        let far = Instant::now() + Duration::from_secs(5);
        queue_rebuilds(&mut st, now, &blocks, &|_| None, far);
        assert!(scan_complete(&st, key(&blocks, &st)), "a whole lap for the new block revision");
        assert!(st.rebuild.contains(&(150, 0)), "the flagged cell the block concerns is queued");
        // Another block: the lap for the old revision no longer counts.
        blocks.set_for_test((-64, -64), 1);
        assert!(!scan_complete(&st, key(&blocks, &st)));
    }

    /// Round 3: room freed late in a frame is seen at once (no baseline from the frame's
    /// start): after a predecessor goes, a refused model that fits now is retried.
    #[test]
    fn room_freed_by_a_replacement_retries_refused_models() {
        let mut st = DistantState::default();
        let mut refused = drawn_cell(1_000_000, None);
        refused.refused_min = Some(500_000);
        st.cells.insert((9, 0), refused);
        st.cells.insert((10, 0), drawn_cell(2_600_000, Some(400_000)));
        retry_refused(&mut st);
        assert!(!st.cells[&(9, 0)].dirty, "4,000,000 of 4,000,000: no room");
        // The replacement at (10, 0) rests: its predecessor's 400,000 go.
        st.cells.get_mut(&(10, 0)).unwrap().old = None;
        retry_refused(&mut st);
        assert!(!st.cells[&(9, 0)].dirty, "400,000 free: still too little for 500,000");
        st.cells.get_mut(&(10, 0)).unwrap().full_of = HashMap::from([(1, 2_400_000)]);
        retry_refused(&mut st);
        assert!(st.cells[&(9, 0)].dirty, "600,000 free: retried in the same frame");
    }

    /// Round 3: a predecessor that drew a reference from its LOD model (its successor no
    /// longer draws it) still counts as drawing it when the reference's override comes.
    #[test]
    fn a_predecessor_drawing_a_lod_model_is_overridden_too() {
        let mut s = drawn_cell(0, None);
        s.plan.insert(7, Choice::Small);
        s.old = Some(Old { entity: Entity::PLACEHOLDER, full_of: HashMap::new(), drawn: HashSet::from([7]) });
        assert!(overridden_here(&s, &HashSet::from([7])), "the predecessor's LOD model");
        assert!(!overridden_here(&s, &HashSet::from([8])));
        s.old = None;
        assert!(!overridden_here(&s, &HashSet::from([7])), "the successor alone draws nothing of it");
        s.plan.insert(7, Choice::Lod);
        assert!(overridden_here(&s, &HashSet::from([7])));
    }

    /// Round 3: a finished fit is compared with the centre this frame judges for, after it
    /// has advanced: a fit for the centre before the move is dropped.
    #[test]
    fn a_fit_for_the_old_centre_is_dropped() {
        assert!(fit_current((0, 0), 12, Some((0, 0)), 12));
        assert!(!fit_current((0, 0), 12, Some((2, 0)), 12), "the centre moved two cells this frame");
        assert!(!fit_current((0, 0), 12, Some((0, 0)), 8), "another reach");
        assert!(!fit_current((0, 0), 12, None, 12));
    }

    fn ring_state(level: f32) -> full_detail::RingCellState {
        full_detail::RingCellState::for_test(level)
    }

    /// A plan judges each entry from the middle cell's centre with its ring's ratio; the
    /// drawn set tells whether a cell must be built again.
    #[test]
    fn plans_follow_the_rule_and_the_drawn_set() {
        let base = Arc::new(BaseObject { form_id: 0x200, sig: *b"STAT", edid: None, model: Some("rocks\\rock.nif".into()), world_model: None, world_alt_textures: vec![], worn_only: false, flags: 0, bounds: None });
        let entry = |fid: u32, x: f32, radius: f32, flagged: bool, lod: bool| Entry {
            p: Placement { form_id: fid, sig: *b"REFR", flags: 0, base: Some(0x200), position: [x, 0.5 * CELL_SIZE, 0.0], rotation: [0.0; 3], scale: 1.0, enable_parent: None, open_by_default: false, teleport: false, emittance: None, imposter: false, primitive: false },
            base: base.clone(),
            full: "meshes\\rocks\\rock.nif".into(),
            lod: lod.then(|| "meshes\\rocks\\rock_lod.nif".into()),
            flagged,
            radius,
            enabled: true,
        };
        // Cell (8, 0) from centre (0, 0): references about 8 cells out.
        let x = 8.5 * CELL_SIZE;
        let list = CellList { entries: vec![entry(1, x, 400.0, false, false), entry(2, x, 100.0, false, false), entry(3, x, 10.0, false, true), entry(4, x, 900.0, true, false)], small: 0, inner: 2 };
        let plan = plan_cell(&list, (8, 0), (0, 0), &[], Some(true), None, &no_skip);
        assert_eq!(plan.iter().map(|p| p.1).collect::<Vec<_>>(), [Choice::Full, Choice::Small, Choice::Lod, Choice::Held]);
        let set = drawn_set(&list, &plan);
        assert_eq!(set.len(), 2);
        // The block not known yet: the flagged one waits.
        assert_eq!(plan_cell(&list, (8, 0), (0, 0), &[], None, None, &no_skip)[3].1, Choice::Unknown);
        // Its segment empty: drawn by size.
        assert_eq!(plan_cell(&list, (8, 0), (0, 0), &[], Some(false), None, &no_skip)[3].1, Choice::Full);
        // A raised ratio for 8 steps leaves the 400-unit rock out unless drawn before.
        let mut ratios = vec![SIZE_RATIO; 9];
        ratios[8] = 0.0135;
        assert_eq!(plan_cell(&list, (8, 0), (0, 0), &ratios, Some(true), None, &no_skip)[0].1, Choice::Small);
        let before = HashMap::from([(1u32, Choice::Full)]);
        assert_eq!(plan_cell(&list, (8, 0), (0, 0), &ratios, Some(true), Some(&before), &no_skip)[0].1, Choice::Full, "hysteresis");
    }

    /// A drawn cell for the lifecycle tests, with its full models' triangles and those of
    /// the cell it replaces.
    fn drawn_cell(full_tris: u64, old: Option<u64>) -> CellState {
        CellState {
            entity: Entity::PLACEHOLDER,
            level: 1.0,
            leaving: false,
            under: false,
            plan: HashMap::new(),
            stamp: Stamp::default(),
            segment: None,
            flagged: false,
            dirty: false,
            full_of: HashMap::from([(1, full_tris)]),
            refused_min: None,
            old: old.map(|t| Old { entity: Entity::PLACEHOLDER, full_of: HashMap::from([(1, t)]), drawn: HashSet::from([1]) }),
            applied: None,
            old_applied: None,
            extent: None,
        }
    }

    fn a_spawn(grid: Grid, replaces: bool, full_tris: u64) -> CellSpawn {
        CellSpawn {
            entity: Entity::PLACEHOLDER,
            grid,
            replaces,
            list: Arc::new(CellList::default()),
            plan: Vec::new(),
            next: 0,
            later: Vec::new(),
            origin: Vec3::ZERO,
            stamp: Stamp::default(),
            segment: None,
            drawn: HashMap::new(),
            full_of: HashMap::from([(1, full_tris)]),
            refused_min: None,
            dirty: false,
        }
    }

    /// The budget equals the sum over the cells alive at every step of a lifecycle: a cell
    /// spawning reserves, a cancelled one gives its share back by leaving, a finished one
    /// retains, a rebuilt one's predecessor counts as fading until it rests, a retired one
    /// counts no more. Nothing is left over once every cell is gone.
    #[test]
    fn the_budget_is_the_sum_over_the_cells_alive() {
        let sum = |st: &DistantState| -> u64 { st.cells.values().map(|s| s.full_tris() + s.old.as_ref().map_or(0, Old::share)).sum::<u64>() + st.spawning.values().map(|s| s.full_of.values().sum::<u64>()).sum::<u64>() };
        let world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut st = DistantState::default();
        // A half-spawned cell that is then dropped (the centre moved): nothing is left.
        st.spawning.insert((9, 0), a_spawn((9, 0), false, 1200));
        assert_eq!(st.budget(), Budget { retained: 0, reserved: 1200, fading: 0 });
        let sp = st.spawning.remove(&(9, 0)).unwrap();
        st.retire(&mut commands, sp.entity);
        assert_eq!(st.budget().total(), 0, "a cancelled cell gives its share back");
        // A finished cell retains its models.
        st.spawning.insert((9, 0), a_spawn((9, 0), false, 3000));
        let sp = st.spawning.remove(&(9, 0)).unwrap();
        finish(&mut commands, &mut st, (9, 0), sp, true, true);
        assert_eq!(st.budget(), Budget { retained: 3000, reserved: 0, fading: 0 });
        assert_eq!(st.budget().total(), sum(&st));
        // Rebuilt in place, crossfading: the predecessor counts as fading.
        st.cells.get_mut(&(9, 0)).unwrap().level = 1.0;
        let sp = a_spawn((9, 0), true, 2500);
        finish(&mut commands, &mut st, (9, 0), sp, true, true);
        assert_eq!(st.budget(), Budget { retained: 2500, reserved: 0, fading: 3000 });
        assert_eq!(st.budget().total(), sum(&st));
        // A rejected rebuild (its cell started leaving) is dropped with its reservation.
        st.cells.get_mut(&(9, 0)).unwrap().leaving = true;
        finish(&mut commands, &mut st, (9, 0), a_spawn((9, 0), true, 700), true, true);
        assert_eq!(st.budget().total(), 5500, "the leaving cell and its predecessor only");
        // Retired with its predecessor: nothing left.
        let s = st.cells.remove(&(9, 0)).unwrap();
        st.retire_cell(&mut commands, s);
        assert_eq!(st.budget(), Budget::default());
        // Cleared with cells and spawns alive: nothing left either.
        st.cells.insert((10, 0), drawn_cell(400, Some(100)));
        st.spawning.insert((11, 0), a_spawn((11, 0), false, 50));
        assert_eq!(st.budget().total(), 550);
        assert_eq!(st.budget().total(), sum(&st));
        let before = st.dying.len();
        st.clear(&mut commands, false);
        assert_eq!(st.budget(), Budget::default());
        assert_eq!(st.dying.len(), before + 3, "indoors or the LOD off: the cell, its predecessor and the spawn hidden now, despawned in slices");
        st.cells.insert((12, 0), drawn_cell(0, None));
        st.clear(&mut commands, true);
        assert!(st.dying.is_empty() && st.cells.is_empty(), "a travel: everything at once");
    }

    /// At the cap a cell rebuilt in place may keep the models of its predecessor (whose
    /// share goes once it rests); a new cell may not pass the cap, and a fading
    /// predecessor counts against every other spawn.
    #[test]
    fn a_replacement_at_the_cap_keeps_its_models() {
        let cap = 4000;
        let b = Budget { retained: 3000, reserved: 0, fading: 0 };
        assert!(admits(b, 3000, 3000, cap), "the rebuilt cell keeps what its predecessor drew");
        assert!(!admits(b, 0, 3000, cap), "a new cell would pass the cap");
        assert!(admits(b, 0, 1000, cap));
        let crossfading = Budget { retained: 3000, reserved: 0, fading: 3000 };
        assert!(!admits(crossfading, 0, 1, cap), "the fading predecessor still counts for other cells");
        assert!(admits(Budget { retained: 3000, reserved: 500, fading: 0 }, 3000, 3500, cap), "reservations count");
        assert!(!admits(Budget { retained: 3000, reserved: 600, fading: 0 }, 3000, 3500, cap));
    }

    /// A cell skipped when the centre moved (fading, covered, replacing) keeps its stale
    /// stamp and is judged again once it is at rest and alone; new ratios alone make every
    /// cell stale; a cell judged for the current stamp is left alone.
    #[test]
    fn a_deferred_cell_is_judged_again_later() {
        let old = Stamp { centre: (0, 0), ratios: 1 };
        let now = Stamp { centre: (2, 0), ratios: 1 };
        let mut s = drawn_cell(0, None);
        s.stamp = old;
        assert!(rejudge_due(&s, now, None, false, false), "at rest and alone: judged now");
        s.level = 0.5;
        assert!(!rejudge_due(&s, now, None, false, false), "fading: later");
        s.level = 1.0;
        assert!(!rejudge_due(&s, now, None, true, false), "covered: later");
        assert!(!rejudge_due(&s, now, None, false, true), "being replaced: later");
        s.old = Some(Old { entity: Entity::PLACEHOLDER, full_of: HashMap::new(), drawn: HashSet::new() });
        assert!(!rejudge_due(&s, now, None, false, false), "replacing: later");
        s.old = None;
        assert!(rejudge_due(&s, now, None, false, false), "caught up once at rest and alone, with no further move");
        s.stamp = now;
        assert!(!rejudge_due(&s, now, None, false, false), "judged for the current stamp");
        assert!(rejudge_due(&s, Stamp { ratios: 2, ..now }, None, false, false), "the fit changed the ratios alone");
        s.leaving = true;
        assert!(!rejudge_due(&s, Stamp { ratios: 2, ..now }, None, false, false), "going: never");
    }

    /// A block arriving late: a cell holding flagged references judged while its block was
    /// unknown is judged again when it becomes known, empty (its flagged references are
    /// then drawn) or holding them (the plan stays and only the answer is recorded); a cell
    /// without flagged references does not care.
    #[test]
    fn a_late_block_rejudges_cells_with_flagged_references() {
        let now = Stamp::default();
        let mut s = drawn_cell(0, None);
        s.flagged = true;
        assert!(!rejudge_due(&s, now, None, false, false), "still unknown");
        assert!(rejudge_due(&s, now, Some(false), false, false), "unknown -> empty");
        assert!(rejudge_due(&s, now, Some(true), false, false), "unknown -> held");
        s.segment = Some(true);
        assert!(!rejudge_due(&s, now, Some(true), false, false), "recorded");
        s.flagged = false;
        s.segment = None;
        assert!(!rejudge_due(&s, now, Some(false), false, false), "no flagged reference");
        // What the plan makes of it (the drawn set decides whether the cell is rebuilt).
        let base = Arc::new(BaseObject { form_id: 0x200, sig: *b"STAT", edid: None, model: Some("rocks\\rock.nif".into()), world_model: None, world_alt_textures: vec![], worn_only: false, flags: 0x8000, bounds: None });
        let e = Entry {
            p: Placement { form_id: 7, sig: *b"REFR", flags: 0, base: Some(0x200), position: [8.5 * CELL_SIZE, 0.5 * CELL_SIZE, 0.0], rotation: [0.0; 3], scale: 1.0, enable_parent: None, open_by_default: false, teleport: false, emittance: None, imposter: false, primitive: false },
            base,
            full: "meshes\\rocks\\rock.nif".into(),
            lod: Some("meshes\\rocks\\rock_lod.nif".into()),
            flagged: true,
            radius: 50.0,
            enabled: true,
        };
        let list = CellList { entries: vec![e], small: 0, inner: 2 };
        let unknown = plan_cell(&list, (8, 0), (0, 0), &[], None, None, &no_skip);
        assert!(drawn_set(&list, &unknown).is_empty(), "unknown: left to the block");
        assert_eq!(drawn_set(&list, &plan_cell(&list, (8, 0), (0, 0), &[], Some(false), None, &no_skip)).get(&7), Some(&Choice::Lod), "empty: drawn, so the cell is rebuilt");
        assert!(drawn_set(&list, &plan_cell(&list, (8, 0), (0, 0), &[], Some(true), None, &no_skip)).is_empty(), "held: the same drawn set, no rebuild");
    }

    /// Model overrides both ways: a reference whose override comes is left out of the plan
    /// (the drawn set changes, so the cell is rebuilt without the original), one whose
    /// override goes is drawn again (the drawn set changes back); the change marks the
    /// cells holding it, which are judged again even while covered or fading.
    #[test]
    fn overrides_applied_and_cleared_rebuild_the_cell() {
        let base = Arc::new(BaseObject { form_id: 0x200, sig: *b"STAT", edid: None, model: Some("rocks\\rock.nif".into()), world_model: None, world_alt_textures: vec![], worn_only: false, flags: 0, bounds: None });
        let e = |fid: u32| Entry {
            p: Placement { form_id: fid, sig: *b"REFR", flags: 0, base: Some(0x200), position: [8.5 * CELL_SIZE, 0.5 * CELL_SIZE, 0.0], rotation: [0.0; 3], scale: 1.0, enable_parent: None, open_by_default: false, teleport: false, emittance: None, imposter: false, primitive: false },
            base: base.clone(),
            full: "meshes\\rocks\\rock.nif".into(),
            lod: Some("meshes\\rocks\\rock_lod.nif".into()),
            flagged: false,
            radius: 50.0,
            enabled: true,
        };
        let list = CellList { entries: vec![e(1), e(2)], small: 0, inner: 2 };
        let plain = drawn_set(&list, &plan_cell(&list, (8, 0), (0, 0), &[], None, None, &no_skip));
        let overridden = |x: &Entry| x.p.form_id == 2;
        let with = drawn_set(&list, &plan_cell(&list, (8, 0), (0, 0), &[], None, None, &overridden));
        assert_eq!(plain.len(), 2);
        assert_eq!(with.keys().copied().collect::<Vec<_>>(), [1], "applied: the original is left out");
        assert_ne!(plain, with, "applied: rebuilt");
        assert_ne!(with, plain, "cleared: rebuilt, drawn again");
        // The changes, both ways.
        let seen: HashSet<u32> = [2, 5].into();
        let now: HashMap<u32, String> = [(5, "a".to_string()), (9, "b".to_string())].into();
        assert_eq!(override_changes(&seen, &now), [2, 9].into(), "2 cleared, 9 applied, 5 unchanged");
        assert!(override_changes(&seen, &[(2, String::new()), (5, String::new())].into()).is_empty());
        // The cells holding them are marked, drawn or being spawned.
        let mut st = DistantState::default();
        let mut holding = drawn_cell(0, None);
        holding.plan.insert(2, Choice::Lod);
        st.cells.insert((8, 0), holding);
        st.cells.insert((9, 0), drawn_cell(0, None));
        let mut sp = a_spawn((10, 0), false, 0);
        sp.list = Arc::new(CellList { entries: vec![e(9)], small: 0, inner: 2 });
        st.spawning.insert((10, 0), sp);
        mark_overridden(&mut st, &[2, 9].into());
        assert!(st.cells[&(8, 0)].dirty && !st.cells[&(9, 0)].dirty && st.spawning[&(10, 0)].dirty);
        let mut s = drawn_cell(0, None);
        s.dirty = true;
        s.level = 0.5;
        assert!(rejudge_due(&s, Stamp::default(), None, true, false), "even fading and covered");
        assert!(!rejudge_due(&s, Stamp::default(), None, true, true), "not while its replacement spawns");
    }

    /// The fit's candidates: references without a LOD model and with a radius, not left to
    /// their block (flagged with a segment holding them or unknown), each with its size over
    /// its distance and the triangles of its model as drawn, or the estimate.
    #[test]
    fn the_fit_reads_the_full_model_candidates() {
        let base = Arc::new(BaseObject { form_id: 0x200, sig: *b"STAT", edid: None, model: None, world_model: None, world_alt_textures: vec![], worn_only: false, flags: 0, bounds: None });
        let e = |fid: u32, radius: f32, flagged: bool, lod: bool, full: &str| Entry {
            p: Placement { form_id: fid, sig: *b"REFR", flags: 0, base: Some(0x200), position: [8.5 * CELL_SIZE, 0.5 * CELL_SIZE, 0.0], rotation: [0.0; 3], scale: 1.0, enable_parent: None, open_by_default: false, teleport: false, emittance: None, imposter: false, primitive: false },
            base: base.clone(),
            full: full.into(),
            lod: lod.then(|| "x_lod.nif".into()),
            flagged,
            radius,
            enabled: true,
        };
        let list = Arc::new(CellList { entries: vec![e(1, 400.0, false, false, "a.nif"), e(2, 400.0, false, true, "b.nif"), e(3, 0.0, false, false, "c.nif"), e(4, 400.0, true, false, "d.nif")], small: 0, inner: 2 });
        let tris: HashMap<String, u64> = [("a.nif".to_string(), 1234)].into();
        let held = candidates(&[(8, Some(true), list.clone())], &tris, (0, 0));
        assert_eq!(held.len(), 1, "the LOD model, the one without radius and the held one left out");
        assert_eq!((held[0].steps, held[0].triangles), (8, 1234));
        assert!((held[0].size - 400.0 / (8.0 * CELL_SIZE)).abs() < 1e-6);
        let empty = candidates(&[(8, Some(false), list.clone())], &tris, (0, 0));
        assert_eq!(empty.len(), 2, "an empty segment: the flagged one is a candidate");
        assert_eq!(empty[1].triangles, UNKNOWN_TRIANGLES);
        assert_eq!(candidates(&[(8, None, list)], &tris, (0, 0)).len(), 1, "unknown: held");
    }

    /// Off in both looks; `--graphics-only distant-objects-8` / `distant-objects-12` turn
    /// it on; the choices name their reaches.
    #[test]
    fn off_in_both_presets_and_the_keys_parse() {
        use crate::graphics::Preset;
        assert_eq!(Graphics::preset(Preset::Vanilla).distant_objects, DistantObjects::Off);
        assert_eq!(Graphics::preset(Preset::Enhanced).distant_objects, DistantObjects::Off);
        assert_eq!(Graphics::only(&["distant-objects-8".into()]).unwrap().distant_objects, DistantObjects::Eight);
        let far = Graphics::only(&["distant-objects-12".into()]).unwrap();
        assert_eq!(far.distant_objects, DistantObjects::Twelve);
        assert_eq!(far.describe(), "light glow, distant objects 12 cells");
        assert!(Graphics::only(&["distant-objects-16".into()]).is_err());
        assert_eq!(DistantObjects::default(), DistantObjects::Off);
        assert_eq!(DistantObjects::CHOICES.map(|(_, d)| d.reach()), [0, REACH_NEAR, REACH_FAR]);
        assert_eq!(DistantObjects::Twelve.name(), "12 cells");
        assert_eq!(DistantObjects::Off.describe(), "");
    }

    /// A distant cell is no loaded or render cell, and the sliders leave its meshes alone.
    #[test]
    fn a_distant_cell_is_no_render_or_loaded_cell() {
        let mut world = World::new();
        world.spawn((DistantCell { grid: (9, 0) }, nvf_play::DrawOnlyCell, Transform::default(), Visibility::Hidden));
        assert_eq!(world.query::<&nvf_play::RenderCell>().iter(&world).count(), 0);
        assert_eq!(world.query_filtered::<(), With<world::CellEntity>>().iter(&world).count(), 0);
        assert_eq!(world.query::<&DistantCell>().iter(&world).count(), 1);
    }
}
