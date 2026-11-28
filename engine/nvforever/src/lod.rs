//! Distant LOD: the game's pre-built terrain LOD chunks and object LOD blocks drawn
//! beyond the loaded cells (docs/spec-notes/world.md §7, docs/design/layer3.md).
//!
//! Observed layout: `meshes\landscape\lod\<world>\<world>.level<L>.x<X>.y<Y>.nif` for
//! L = 4, 8, 16, 32 covers the L × L cells from (X, Y), its root
//! node translated to the chunk's south-west corner, with its own 256 × 256 diffuse and
//! normal textures at every level; object LOD blocks exist at level 4 only
//! (`...\blocks\<world>.level4.x<X>.y<Y>.nif`). X and Y are the world's LOD origin
//! plus multiples of L: the south-west cell its `lodsettings\<world>.dlodsettings` names
//! ([`grid_origin`]). WastelandNV's is (-64, -64), a multiple of every level; Freeside's
//! is (-10, -8), so its files are x-10, x-6, x-2, x2 … [observed: the archives' file
//! names and the settings files; #1533: aligned to multiples of 4 there, no block or game
//! chunk was found, and from Freeside the Strip's towers showed only their lights].
//! Near the camera, chunks generated from
//! the LAND records (`lodgen.rs`, docs/design/generated-lod.md) replace the game's
//! level-4 terrain chunks once baked or read from NVForever's cache.
//!
//! Which chunks are drawn [hypothesis; names and values from the owner's
//! `FalloutPrefs.ini`, meanings not documented, layer3.md "Distant LOD"]: a quadtree over
//! the level-32 grid. No terrain chunk farther than the terrain reach
//! ([`LodSettings::terrain_reach`]: [`LodSettings::terrain_far`], 250,000, or
//! [`LodSettings::far`] when that is larger) is drawn; a chunk coarser than level 4 is
//! replaced by its four quarters while the camera is nearer (horizontally, to the
//! chunk's closest point) than max([`LodSettings::near`] (fBlockLoadDistanceLow),
//! [`LodSettings::split`] (fSplitDistanceMult) × the chunk's side). Every level has 256
//! texels per chunk side, so each level takes over where its texels look about as large
//! as the finer level's did at its own limit. At the owner's values level 4 reaches
//! 50,000 units, level 8 98,304, level 16 196,608 and level 32 the rest of the 250,000.
//! The two INI values agree on where level 4 ends at the "Very High" (8 cells × 1.5 =
//! 49,152 against 50,000) and "Low" presets (24,576 against 25,000), not at "Medium" and
//! "High" (32,768 against 50,000) [observed: `low.ini` … `VeryHigh.ini` in the game
//! folder]. The choice is redone when the camera enters another cell; levels switch at
//! once. Object blocks are drawn over every 4 × 4-cell square nearer than `far`
//! (fBlockLoadDistance, 125,000), whatever the terrain level there. The terrain's reach
//! of 250,000 is a calibrated default: at one pose, the hill above Goodsprings (-65347,
//! 5770, z 13970, heading 27°), our render's skyline matched the original's from the
//! first tested `--lod-distance` of 250,000, and 150,000, 175,000 and 200,000 did not
//! (that sweep, before the two reaches were split, also moved the object blocks and the
//! distant water) [observed: capture of the original and our runs, 2026-11-19, #3055].
//! How far the original actually draws terrain, and whether it is one reach for every
//! place and preset, is [hypothesis]; no INI key gives 250,000 [observed: the game's INI
//! files]. Distant water (water.rs) covers the cells under the chosen chunks
//! ([`LodState::chosen_chunks`]).
//!
//! Neighbouring chunks do not share their edge vertices, at the same level or across
//! levels [observed: level4.x8.y8 / x12.y8 share 14 of 16 + 52 edge positions], so every
//! chunk gets skirts: its open edges hang down by [`skirt_depth`], hiding the cracks.
//!
//! Interim rules [hypothesis, to compare with screenshots]: terrain chunks are drawn with
//! a negative depth bias so loaded terrain wins where both exist, and where a chunk
//! overlaps cells that are fully drawn only its triangles reaching outside them are kept,
//! with the vertices inside sunk below the loaded terrain. LOD gives way to a cell only
//! once the cell is actually loaded and has faded in (fade.rs), never ahead of it: cells
//! stream in one per frame, and cutting the LOD for the cells merely wanted left holes at
//! the edge of the loaded area until they arrived. In a square with loaded cells the
//! object block is split by cell, and each cell's part crossfades with the cell;
//! elsewhere the block is drawn whole. The split is the block's own: its shape is a
//! BSSegmentedTriShape whose segment i holds the objects of cell (i / 4, i mod 4) of the
//! square, each object whole, also where it reaches into the next cell [observed: no
//! vertex is used by two segments and none of the 544,846 connected pieces of the 355
//! block shapes lies in two, though 23,121 reach over more than one cell; world.md §7
//! "Objects"]. So an object's LOD goes exactly when the cell that draws its full model
//! has loaded [hypothesis that the game hides a loaded cell's segment]. Splitting by
//! triangle centre instead cut such objects in two at the edge of the loaded cells: the
//! cliffs on the peak north-west of Goodsprings, placed in cell (-20, 1), lost the part
//! inside cell (-19, 1) while their own cell was not loaded [observed: owner's frames,
//! 2026-10-10].
//! Blocks are alpha-tested: their atlas cuts trees, pylons and lattices out in its alpha
//! channel, though the meshes have no NiAlphaProperty (world.md §7 "Objects"; the model
//! loader does it for the "LOD_Building" shader flag, `nif_spawn::shape_alpha_mode`).
//! A block's model (with its CPU meshes, which the split needs) is prepared on a worker
//! thread (`ModelCtx::model_with_cpu_ready`); its square waits until it is, as squares
//! wait for the rebuild budget (#1048). So does a game terrain chunk's model.
//!
//! Terrain chunk shading [observed, world.md §7]: the chunk meshes carry no vertex
//! normals and their normal maps are in model space (red = east, green = north, blue =
//! up), so each chunk mesh gets a constant up normal and an east tangent, which makes
//! the renderer's tangent-space normal mapping read the map as model-space normals. The
//! untextured second shape in each chunk (flat at the water height) is not drawn; the
//! water planes cover it [hypothesis: a water LOD surface].
//!
//! Diffuse textures (#3096) [design]: a game terrain chunk is spawned, and counts as
//! covering the chunks it replaces, only once no shape it draws waits for its diffuse
//! ([`Surface`]); until then the old chunks stay drawn. A shape whose diffuse is missing
//! (not named, or its file failed to load, a failure that is not retried) would draw
//! white, so it is drawn with a neutral fallback colour instead ([`fallback_material`]:
//! a copy owned by the shape's entity alone, released with it), logged once per chunk
//! when spawned, with the texture's path and error, and the chunk counts as covering like
//! any other. A player's report showed distant hills flat white at night
//! [observed: report R-ZXT6AC4J's screenshot]; its cause is not confirmed.

use crate::fade;
use crate::lodgen::{self, Baker, GenJob};
use crate::world::{bevy_to_game, WorldData, WorldRoot};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use fnv_world::{cell_of, CELL_SIZE};
use nvf_assets::model::ModelCtx;
use nvf_assets::texture::dds_image;
use nvf_assets::{nif_material, NifMaterial, PreparedModel, PreparedShape};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Terrain LOD levels in the data, finest first [observed: WastelandNV has 1,024 / 256 /
/// 64 / 16 chunks; its `.dlodsettings` reads 4 and 32 as the smallest and largest].
pub const LEVELS: [i32; 4] = [4, 8, 16, 32];
/// The finest level: object blocks and generated terrain use its squares.
const LEVEL: i32 = LEVELS[0];
/// Cells in a level-4 square (bit `y * LEVEL + x` of the masks below).
const CELLS: usize = (LEVEL * LEVEL) as usize;

type Grid = (i32, i32);
/// A terrain chunk: level, then its south-west cell.
pub type Chunk = (i32, i32, i32);
/// Each cell's part of a square's object block: mesh, material, transform.
type Pieces = Vec<Vec<(Handle<Mesh>, Handle<NifMaterial>, Transform)>>;

/// Distances that choose what distant LOD is drawn (game units). The defaults are the
/// owner's `FalloutPrefs.ini` values (the game's "Very High" preset, which ships as
/// `VeryHigh.ini` next to the game) [observed]; how the game uses them is
/// [hypothesis] (module docs, layer3.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodSettings {
    /// `[TerrainManager] fBlockLoadDistance`: no object block beyond (and the terrain
    /// reaches at least this far, [`LodSettings::terrain_reach`]).
    pub far: f32,
    /// No terrain chunk beyond this (or beyond `far`, when that is larger). The default,
    /// 250,000, is calibrated at one pose, the hill above Goodsprings (-65347, 5770,
    /// z 13970, heading 27°): our render's skyline matched the original's from the first
    /// tested `--lod-distance` of 250,000; 150,000, 175,000 and 200,000 did not (that
    /// sweep also moved the object blocks and the distant water) [observed: capture of
    /// the original and our runs, 2026-11-19, #3055]. How far the original draws terrain
    /// is [hypothesis], as is one fixed number for every place and preset; the game's INI
    /// files hold no `[TerrainManager]` key that gives it [observed: `Fallout_default.ini`
    /// and the four presets].
    pub terrain_far: f32,
    /// `[TerrainManager] fBlockLoadDistanceLow`: level-4 terrain chunks within.
    pub near: f32,
    /// `[TerrainManager] fSplitDistanceMult`: coarser chunks split while the camera is
    /// nearer than this × their side.
    pub split: f32,
    /// `[TerrainManager] fTreeLoadDistance`: distant tree billboards within at the start
    /// (treelod.rs; the Tree distance slider then sets a part of `far`, #3016); 0 draws none.
    pub trees: f32,
}

/// [`LodSettings::far`]'s default (`--lod-distance`).
pub const DEFAULT_FAR: f32 = 125_000.0;

impl Default for LodSettings {
    fn default() -> LodSettings {
        LodSettings { far: DEFAULT_FAR, terrain_far: 250_000.0, near: 50_000.0, split: 1.5, trees: 40_000.0 }
    }
}

impl LodSettings {
    /// How far terrain chunks are drawn: `terrain_far`, or `far` when that is larger
    /// (the terrain never stops short of the object blocks standing on it).
    pub fn terrain_reach(&self) -> f32 {
        self.terrain_far.max(self.far)
    }
}

/// Horizontal distance from `p` (game x, y) to the closest point of the square of
/// `n` × `n` cells whose south-west cell is `c` (0 inside it).
pub fn square_distance(p: Vec2, (x, y): Grid, n: i32) -> f32 {
    let (x0, y0, side) = (x as f32 * CELL_SIZE, y as f32 * CELL_SIZE, n as f32 * CELL_SIZE);
    let dx = (x0 - p.x).max(p.x - (x0 + side)).max(0.0);
    let dy = (y0 - p.y).max(p.y - (y0 + side)).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Level-4 squares (of 4 × 4 cells) it takes to reach `d` game units (0 for none).
pub fn squares_within(d: f32) -> i32 {
    (d / (LEVEL as f32 * CELL_SIZE)).ceil().max(0.0) as i32
}

/// The terrain chunks to draw for a camera at `p` (module docs): leaves of a quadtree
/// over the coarsest level, none of them overlapping, nearest level 4.
pub fn select_chunks(p: Vec2, s: &LodSettings, origin: Grid) -> Vec<Chunk> {
    let top = LEVELS[LEVELS.len() - 1];
    let mut out = Vec::new();
    let reach = s.terrain_reach();
    if reach <= 0.0 {
        return out;
    }
    let r = (reach / CELL_SIZE).ceil() as i32 + top;
    let (cx, cy) = cell_of(p.x, p.y);
    for x in (align(cx - r, origin.0, top)..=cx + r).step_by(top as usize) {
        for y in (align(cy - r, origin.1, top)..=cy + r).step_by(top as usize) {
            visit(p, s, reach, (top, x, y), &mut out);
        }
    }
    out
}

fn visit(p: Vec2, s: &LodSettings, reach: f32, (l, x, y): Chunk, out: &mut Vec<Chunk>) {
    let d = square_distance(p, (x, y), l);
    if d >= reach {
        return;
    }
    if l > LEVEL && d < s.near.max(s.split * l as f32 * CELL_SIZE) {
        let h = l / 2;
        for (dx, dy) in [(0, 0), (h, 0), (0, h), (h, h)] {
            visit(p, s, reach, (h, x + dx, y + dy), out);
        }
    } else {
        out.push((l, x, y));
    }
}

/// The level-4 squares (south-west cells, on the grid from `origin`) whose object blocks
/// are drawn for a camera at `p`: those nearer than `far` (not the terrain's reach,
/// module docs), nearest first, so squares put off by the rebuild budget are the far ones.
pub fn block_squares(p: Vec2, s: &LodSettings, origin: Grid) -> Vec<Grid> {
    let (cx, cy) = cell_of(p.x, p.y);
    let centre = ((cx - origin.0).div_euclid(LEVEL), (cy - origin.1).div_euclid(LEVEL));
    let r = squares_within(s.far) + 1;
    let mut out: Vec<Grid> = (-r..=r)
        .flat_map(|dx| (-r..=r).map(move |dy| (origin.0 + (centre.0 + dx) * LEVEL, origin.1 + (centre.1 + dy) * LEVEL)))
        .filter(|&sq| square_distance(p, sq, LEVEL) < s.far)
        .collect();
    out.sort_by(|&a, &b| square_distance(p, a, LEVEL).total_cmp(&square_distance(p, b, LEVEL)));
    out
}

/// The first cell of the level-`l` chunk holding cell `c`, on a grid starting at `o`.
fn align(c: i32, o: i32, l: i32) -> i32 {
    o + (c - o).div_euclid(l) * l
}

/// The world's LOD grid origin: the south-west cell of `lodsettings\<world>.dlodsettings`
/// (bytes 12–15, two i16 [observed: WastelandNV -64, -64; FreesideWorld -10, -8;
/// FreesideNorthWorld -9, -7; world.md §7]); (0, 0) without the file.
pub fn grid_origin(game: &nvf_assets::GameData, world: &WorldData) -> Grid {
    // A child with Use LOD Data shows its parent's files (`WorldIndex::lod_world`, #2345).
    let name = world.index.worldspaces.get(&world.index.lod_world(world.world)).and_then(|w| w.edid.clone()).unwrap_or_default().to_ascii_lowercase();
    game.read(&format!("lodsettings\\{name}.dlodsettings")).as_deref().and_then(settings_origin).unwrap_or((0, 0))
}

/// The south-west cell a `.dlodsettings` file names.
fn settings_origin(b: &[u8]) -> Option<Grid> {
    let i = |k: usize| b.get(k..k + 2).map(|s| i16::from_le_bytes([s[0], s[1]]) as i32);
    Some((i(12)?, i(14)?))
}

/// Skirt depth for a chunk of level `l` (game units) [chosen: deep enough to cover the
/// height steps seen between neighbouring chunks, 128 units per cell of chunk side].
pub fn skirt_depth(l: i32) -> f32 {
    128.0 * l as f32
}

/// What a terrain chunk was drawn for.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TerrainKey {
    /// Cells inside the chunk that are fully drawn (loaded, not fading): cut out there.
    full: Vec<Grid>,
    /// Generated terrain instead of the game's (level 4 only).
    generated: bool,
}

/// What an object block was drawn for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct BlockKey {
    /// Cells fully drawn: the block's part there is left out.
    full: u16,
    /// Cells loaded at all: the block is drawn per cell.
    present: u16,
}

/// One terrain chunk's entities.
struct TerrainEntities {
    entities: Vec<Entity>,
    key: TerrainKey,
    /// Drawn: `Some(true)` generated, `Some(false)` the game's, `None` nothing (no file).
    drawn: Option<bool>,
}

/// One object block's entities.
#[derive(Default)]
struct BlockEntities {
    entities: Vec<Entity>,
    /// The per-cell parts: (cell index, entity).
    pieces: Vec<(usize, Entity)>,
    /// The fade step each cell's parts were given (fade.rs).
    steps: [u8; CELLS],
    key: BlockKey,
    drawn: bool,
}

fn despawn_all(commands: &mut Commands, entities: impl IntoIterator<Item = Entity>) {
    for e in entities {
        commands.entity(e).despawn();
    }
}

#[derive(Resource)]
pub struct LodState {
    pub settings: LodSettings,
    pub enabled: bool,
    world_name: String,
    /// The south-west cell of the world's LOD grid ([`grid_origin`]).
    origin: Grid,
    /// The camera's cell the drawn chunks were chosen for.
    centre: Option<Grid>,
    /// Terrain chunks and object-block squares chosen for `centre`.
    chosen: Vec<Chunk>,
    squares: Vec<Grid>,
    /// Counts the times `chosen` was replaced (water.rs redoes its cells when it moves).
    choice: u64,
    terrain: HashMap<Chunk, TerrainEntities>,
    /// Chunks no longer chosen, kept drawn until every chosen chunk over their area is
    /// spawned: those wait for their models on worker threads, and dropping the old ones
    /// at once left holes in the distant terrain meanwhile (#1052 review).
    outgoing: HashMap<Chunk, TerrainEntities>,
    block_entities: HashMap<Grid, BlockEntities>,
    /// Shading-fixed meshes of whole (untrimmed) game terrain chunks, per chunk and shape.
    meshes: HashMap<Chunk, Vec<Option<Handle<Mesh>>>>,
    /// Object blocks split by cell, for squares with loaded cells.
    pieces: HashMap<Grid, Pieces>,
    /// Frames in a row each square still waiting for its new or changed block was put off
    /// by the budget (cleared when its block goes in or it leaves the range).
    put_off_runs: HashMap<Grid, usize>,
    /// Generated terrain near the camera (docs/design/generated-lod.md); `None`: off.
    pub gen: Option<GenState>,
    /// Distant tree billboards (treelod.rs).
    pub trees: crate::treelod::DistantTrees,
    /// Game terrain chunks drawn (and per level, finest first), generated terrain
    /// chunks and object blocks drawn.
    pub chunks: usize,
    pub levels: [usize; 4],
    pub generated: usize,
    pub blocks: usize,
    /// Terrain chunks spawned anew since the start (shadow_probe.rs).
    pub terrain_rebuilds: usize,
    /// What the object-block loop did since the start (logged at the exit).
    pub block_stats: BlockStats,
    /// What the diffuse checks did since the start (#3096, logged at the exit).
    pub surfaces: SurfaceStats,
    /// Chunks already logged as drawn with the fallback colour, and as drawn while a
    /// diffuse was still on its way.
    warned: HashSet<Chunk>,
    unguarded: HashSet<Chunk>,
}

/// Object-block work over a run, for the exit log (stress runs and smoke read it).
#[derive(Default, Clone, Copy, Debug)]
pub struct BlockStats {
    /// Squares that got their first block, and blocks rebuilt for changed cells.
    pub spawned: usize,
    pub rebuilt: usize,
    /// Most blocks spawned or rebuilt in one frame.
    pub most_in_frame: usize,
    /// Squares put off to a later frame because the frame's budget was spent (one per
    /// square and frame).
    pub put_off: usize,
    /// The most frames in a row one square was put off (#1058).
    pub longest_put_off: usize,
    /// Squares in range still without their new or changed block after the last frame
    /// (put off, or waiting for their model).
    pub waiting_now: usize,
    /// Squares that waited a frame for their block's model, prepared on a worker thread
    /// (one per square and frame).
    pub waited: usize,
    /// The same for terrain chunks (one per chunk and frame).
    pub terrain_waited: usize,
    /// Frames that kept outgoing terrain chunks drawn until their replacements were in,
    /// and the most kept at once.
    pub outgoing_frames: usize,
    pub outgoing_most: usize,
    /// Slowest frames of the whole LOD update and of its block loop alone (ms), and the
    /// frames the update took longer than [`SLOW_UPDATE_MS`].
    pub update_slowest_ms: f32,
    pub blocks_slowest_ms: f32,
    pub slow_updates: usize,
}

/// The diffuse checks of game terrain chunks over a run (#3096), for the exit log.
#[derive(Default, Clone, Copy, Debug)]
pub struct SurfaceStats {
    /// Chunks drawn with the fallback colour for a missing diffuse (each counted once), and
    /// the shapes spawned with it.
    pub fallback_chunks: usize,
    pub fallback_shapes: usize,
    /// Frames a chunk waited for its diffuse to reach the image store (one per chunk and
    /// frame).
    pub texture_waits: usize,
    /// Terrain shapes spawned while their diffuse was still on its way (0 expected).
    pub drawn_without_diffuse: usize,
}

impl SurfaceStats {
    pub fn describe(&self) -> String {
        format!(
            "{} chunks drawn with the fallback colour ({} shapes), {} waits for a chunk's textures, {} shapes drawn without a diffuse texture",
            self.fallback_chunks, self.fallback_shapes, self.texture_waits, self.drawn_without_diffuse
        )
    }
}

/// An LOD update this long (ms) counts as slow in [`BlockStats`]: twice the budget.
const SLOW_UPDATE_MS: f32 = 2.0 * REBUILD_BUDGET_MS;

impl BlockStats {
    pub fn describe(&self) -> String {
        format!(
            "{} new, {} rebuilt, up to {} in one frame, {} put off to a later frame, {} waited for their model; slowest update {:.1} ms (blocks {:.1} ms), {} updates over {:.0} ms; terrain chunks waited {} times for their model, outgoing chunks kept drawn in {} frames (up to {} at once); a square put off up to {} frames in a row, {} squares still waiting for their block at the end",
            self.spawned, self.rebuilt, self.most_in_frame, self.put_off, self.waited, self.update_slowest_ms, self.blocks_slowest_ms, self.slow_updates, SLOW_UPDATE_MS, self.terrain_waited, self.outgoing_frames, self.outgoing_most, self.longest_put_off, self.waiting_now
        )
    }
}

impl LodState {
    /// The terrain chunks' entities (shadow_probe.rs).
    pub fn terrain_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.terrain.values().chain(self.outgoing.values()).flat_map(|t| t.entities.iter().copied())
    }

    /// The terrain chunks chosen for the camera's cell ([`select_chunks`]; none while
    /// distant LOD is off or before the first update). Distant water (water.rs) covers
    /// the cells under them, so it ends where the terrain does.
    pub fn chosen_chunks(&self) -> &[Chunk] {
        &self.chosen
    }

    /// Changes whenever [`LodState::chosen_chunks`] is replaced.
    pub fn choice(&self) -> u64 {
        self.choice
    }

    /// Tests: the chunks `update_lod` would have chosen (counts as a new choice).
    #[cfg(test)]
    pub fn choose_for_test(&mut self, chunks: Vec<Chunk>) {
        self.chosen = chunks;
        self.choice += 1;
    }
}

/// What each level-4 square's object block holds, by cell: bit `y * 4 + x` of its mask
/// ([`chunk_bit`]) is set when the block's segment for that cell is not empty (#3014; read
/// by `crate::distant_objects`, which leaves a reference to the block when the block may
/// hold it). A square is known once its block has been prepared for drawing
/// ([`update_lod`]): a square without a block file, or whose file did not load, holds
/// nothing (mask 0); a block shape without usable segments counts as holding every cell
/// ([`segment_mask`]). Unknown until then [design].
#[derive(Resource, Default)]
pub struct BlockContents {
    /// The LOD files' worldspace name and grid origin the masks are of.
    world: String,
    origin: Grid,
    squares: HashMap<Grid, u16>,
    /// Counts the changes (a settled distant frame compares it).
    rev: u64,
}

impl BlockContents {
    /// Whether the object block of cell `c`'s square has a non-empty segment for `c`;
    /// `None` while that block is not known yet.
    pub fn segment(&self, c: Grid) -> Option<bool> {
        let (sq, bit) = chunk_bit(c, self.origin);
        self.squares.get(&sq).map(|m| m & bit != 0)
    }

    /// Square `sq` holds the cells of `mask`, for other modules' tests.
    #[cfg(test)]
    pub(crate) fn set_for_test(&mut self, sq: Grid, mask: u16) {
        self.set(sq, mask);
    }

    /// Counts the changes of the masks.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// The masks known (squares).
    #[cfg(test)]
    pub fn known(&self) -> usize {
        self.squares.len()
    }

    /// Forget everything when the LOD files change (another worldspace's).
    fn follow(&mut self, world: &str, origin: Grid) {
        if self.world != world || self.origin != origin {
            self.world = world.to_string();
            self.origin = origin;
            self.squares.clear();
            self.rev += 1;
        }
    }

    /// Square `sq` (its south-west cell) holds the cells of `mask`.
    fn set(&mut self, sq: Grid, mask: u16) {
        if self.squares.insert(sq, mask) != Some(mask) {
            self.rev += 1;
        }
    }
}

/// The cells of a level-4 square an object block holds anything for ([`BlockContents`]),
/// from its shapes' segments (triangle ranges, one per cell in [`segment_cell`] order): a
/// non-empty segment sets its cell's bit; a shape with no segments or more than the
/// square has cells sets every bit (its objects cannot be told apart by cell).
pub fn segment_mask<'a>(shapes: impl IntoIterator<Item = &'a [std::ops::Range<usize>]>) -> u16 {
    let mut mask = 0u16;
    for segments in shapes {
        if segments.is_empty() || segments.len() > CELLS {
            return u16::MAX;
        }
        for (i, r) in segments.iter().enumerate() {
            if !r.is_empty() {
                mask |= 1 << segment_cell(i);
            }
        }
    }
    mask
}

/// Threads baking generated terrain (lodgen::Baker, at the lowest priority).
const BAKE_THREADS: usize = 2;
/// Finished bakes taken per frame: each uploads two textures and builds a mesh on the main
/// thread, and cached chunks finish many at once.
const BAKES_PER_FRAME: usize = 2;

/// Main-thread time (ms) a frame spends rebuilding terrain chunks and object blocks for
/// the loaded cells; the rest wait for the next frames, the old meshes drawn meanwhile (the
/// LOD terrain sits behind loaded terrain by its depth bias). Crossing a cell rebuilt every
/// changed chunk in one frame, 60–71 ms (`lod::update_lod` in `profile hitch` lines,
/// tools/stress tour Goodsprings–Primm, 2026-10-27) [observed]; the budget is an
/// engineering choice. At least one terrain chunk and at least one object block go through
/// each frame (when one is waiting with its model ready), whatever the other spent, so both
/// always move on: blocks come after terrain, and terrain alone used to spend the budget
/// for them while flying fast (#1058).
const REBUILD_BUDGET_MS: f32 = 4.0;

/// Generated terrain chunks: bakes in flight, chunks ready to draw.
pub struct GenState {
    pub job: GenJob,
    /// Generated chunks replace the game's level-4 chunks within this many squares of the
    /// camera's square.
    pub radius: i32,
    baker: Baker,
    /// Chunks handed to the baker whose results have not been taken.
    pending: HashSet<Grid>,
    ready: HashMap<Grid, GenChunk>,
    /// Chunks without LAND or whose bake failed: the game's chunk is drawn.
    skip: HashSet<Grid>,
    /// Chunks baked this run, and read from the cache.
    pub baked: usize,
    pub cached: usize,
}

impl GenState {
    pub fn new(job: GenJob, radius: i32) -> GenState {
        let baker = Baker::new(job.clone(), BAKE_THREADS);
        GenState { job, radius, baker, pending: HashSet::new(), ready: HashMap::new(), skip: HashSet::new(), baked: 0, cached: 0 }
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

struct GenChunk {
    heights: Vec<Option<Vec<f32>>>,
    material: Handle<NifMaterial>,
    /// The mesh with every cell, for chunks clear of the loaded cells.
    full: Option<Handle<Mesh>>,
}

impl LodState {
    pub fn new(world: &WorldData, settings: LodSettings, enabled: bool, gen: Option<GenState>) -> LodState {
        // A child with Use LOD Data shows its parent's files (`WorldIndex::lod_world`, #2345).
        let name = world.index.worldspaces[&world.index.lod_world(world.world)].edid.clone().unwrap_or_default().to_ascii_lowercase();
        LodState {
            settings,
            enabled,
            world_name: name,
            origin: (0, 0),
            centre: None,
            chosen: Vec::new(),
            squares: Vec::new(),
            choice: 0,
            terrain: HashMap::new(),
            outgoing: HashMap::new(),
            block_entities: HashMap::new(),
            meshes: HashMap::new(),
            pieces: HashMap::new(),
            put_off_runs: HashMap::new(),
            gen,
            trees: Default::default(),
            chunks: 0,
            levels: [0; 4],
            generated: 0,
            blocks: 0,
            terrain_rebuilds: 0,
            block_stats: BlockStats::default(),
            surfaces: SurfaceStats::default(),
            warned: HashSet::new(),
            unguarded: HashSet::new(),
        }
    }

    /// The world's LOD grid starts at `origin` ([`grid_origin`]).
    pub fn with_origin(mut self, origin: Grid) -> LodState {
        self.origin = origin;
        self
    }

    /// Generated chunks still baking (0 when generation is off).
    pub fn gen_pending(&self) -> usize {
        self.gen.as_ref().map_or(0, GenState::pending)
    }

    /// Turn distant LOD on or off (off: its entities are removed now).
    pub fn set_enabled(&mut self, commands: &mut Commands, enabled: bool) {
        self.enabled = enabled;
        self.centre = None;
        self.chosen.clear();
        self.choice += 1;
        for (_, t) in self.terrain.drain().chain(self.outgoing.drain()) {
            despawn_all(commands, t.entities);
        }
        for (_, b) in self.block_entities.drain() {
            despawn_all(commands, b.entities.into_iter().chain(b.pieces.into_iter().map(|(_, e)| e)));
        }
        self.trees.clear(commands);
        self.chunks = 0;
        self.levels = [0; 4];
        self.generated = 0;
        self.blocks = 0;
    }

    /// Terrain chunks drawn per level, for the overlay and logs.
    pub fn describe(&self) -> String {
        let l = self.levels;
        format!("LOD chunks {} (levels 4/8/16/32: {}/{}/{}/{}) generated {} (baking {}) blocks {} trees {}", self.chunks, l[0], l[1], l[2], l[3], self.generated, self.gen_pending(), self.blocks, self.trees.count)
    }

    fn terrain_path(&self, (l, x, y): Chunk) -> String {
        format!("meshes\\landscape\\lod\\{0}\\{0}.level{l}.x{x}.y{y}.nif", self.world_name)
    }

    fn block_path(&self, x: i32, y: i32) -> String {
        format!("meshes\\landscape\\lod\\{0}\\blocks\\{0}.level{LEVEL}.x{x}.y{y}.nif", self.world_name)
    }
}

/// Strictly inside the cells of `full` (game x/y): every cell whose closed square holds
/// the point is in it, so points on the outer edge of those cells are outside.
fn inside(p: Vec3, full: &HashSet<Grid>) -> bool {
    let touching = |v: f32| {
        let f = (v / CELL_SIZE).floor();
        let i = f as i32;
        if f * CELL_SIZE == v { (i - 1, i) } else { (i, i) }
    };
    let ((x0, x1), (y0, y1)) = (touching(p.x), touching(p.y));
    (x0..=x1).all(|x| (y0..=y1).all(|y| full.contains(&(x, y))))
}

/// The level-4 square holding a cell (on the grid from `o`) and the cell's bit in the
/// square masks.
fn chunk_bit(c: Grid, o: Grid) -> (Grid, u16) {
    let ch = (align(c.0, o.0, LEVEL), align(c.1, o.1, LEVEL));
    (ch, 1 << ((c.1 - ch.1) * LEVEL + (c.0 - ch.0)))
}

/// Cell `i` of a level-4 square.
fn chunk_cell((cx, cy): Grid, i: usize) -> Grid {
    (cx + i as i32 % LEVEL, cy + i as i32 / LEVEL)
}

/// Whether cell `c` lies in chunk `(l, x, y)`.
fn in_chunk(c: Grid, (l, x, y): Chunk) -> bool {
    (x..x + l).contains(&c.0) && (y..y + l).contains(&c.1)
}

/// Whether two chunks share any cell.
fn chunks_overlap((la, xa, ya): Chunk, (lb, xb, yb): Chunk) -> bool {
    xa < xb + lb && xb < xa + la && ya < yb + lb && yb < ya + la
}

/// Depth by which LOD vertices inside the loaded square are sunk (game units).
const SINK: f32 = 2000.0;

/// The loaded cells to cut out of a chunk: chunk-to-world transform, fully drawn cells.
type Trim<'a> = (&'a Affine3A, &'a HashSet<Grid>);

/// A drawable copy of a terrain chunk shape: up normals and east tangents so the
/// model-space normal map shades it (module docs), or `None` for the untextured shape.
/// With `trim`, the triangles that lie entirely inside the loaded cells are dropped and
/// vertices inside them are sunk by [`SINK`], so the triangles crossing their edge slope
/// down under the loaded terrain instead of poking through it. Open edges get skirts
/// `skirt` deep (0: none).
fn terrain_mesh(mesh: &Mesh, trim: Option<Trim>, skirt: f32) -> Option<Mesh> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    mesh.attribute(Mesh::ATTRIBUTE_UV_0)?;
    let mut idx: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|&v| v as u32).collect(),
    };
    let mut new_pos = pos.clone();
    if let Some((to_world, full)) = trim {
        let from_world = to_world.inverse();
        let mut within = Vec::with_capacity(pos.len());
        for (p, out) in pos.iter().zip(&mut new_pos) {
            let w = to_world.transform_point3(Vec3::from_array(*p));
            let i = inside(w, full);
            within.push(i);
            if i {
                *out = from_world.transform_point3(w - Vec3::Z * SINK).to_array();
            }
        }
        idx = idx.chunks(3).filter(|t| t.iter().any(|&v| !within[v as usize])).flatten().copied().collect();
        if idx.is_empty() {
            return None;
        }
    }
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    for (id, values) in mesh.attributes() {
        out.insert_attribute(*id, values.clone());
    }
    let first_skirt = new_pos.len();
    let mut copies: Vec<usize> = Vec::new();
    if skirt > 0.0 {
        for (a, b) in open_edges(&idx) {
            let (a2, b2) = ((new_pos.len()) as u32, (new_pos.len() + 1) as u32);
            for v in [a, b] {
                let p = new_pos[v as usize];
                new_pos.push([p[0], p[1], p[2] - skirt]);
                copies.push(v as usize);
            }
            // Faces outwards: a → b runs with the mesh on the left (as lodgen.rs).
            idx.extend_from_slice(&[a, a2, b2, a, b2, b]);
        }
    }
    if !copies.is_empty() {
        // The hanging vertices repeat their edge vertex's attributes (UVs).
        let used: Vec<usize> = (0..first_skirt).chain(copies.iter().copied()).collect();
        for (id, values) in mesh.attributes() {
            out.insert_attribute(*id, gather(values, &used));
        }
    }
    let n = new_pos.len();
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, new_pos);
    // Model space is game space here (the chunk's transforms only translate it).
    out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    // Bitangent = w × (normal × tangent) = up × east = north.
    out.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, 1.0]; n]);
    out.insert_indices(Indices::U32(idx));
    Some(out)
}

/// Edges used by one triangle only, in that triangle's winding order (the triangle on
/// the left of a → b for counter-clockwise triangles), in a stable order.
fn open_edges(idx: &[u32]) -> Vec<(u32, u32)> {
    let mut count: HashMap<(u32, u32), u32> = HashMap::new();
    for t in idx.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for t in idx.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if count[&(a.min(b), a.max(b))] == 1 {
                out.push((a, b));
            }
        }
    }
    out
}

/// Whether [`terrain_mesh`] draws a shape with this mesh: positions, UVs and a triangle
/// list (the untextured water shape has no UVs).
fn drawable(mesh: &Mesh) -> bool {
    matches!(mesh.attribute(Mesh::ATTRIBUTE_POSITION), Some(VertexAttributeValues::Float32x3(_))) && mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some() && mesh.indices().is_some()
}

/// Where a terrain shape's diffuse texture stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Diffuse {
    /// In the image store: the shape draws textured.
    Ready,
    /// Named, not in the image store yet (decoded on a worker, put in by the main thread);
    /// the renderer skips such a material until it is.
    Loading,
    /// None: not named, or it failed to load (cached, not tried again). The material
    /// would draw with Bevy's white fallback image.
    Missing,
}

fn diffuse_of(material: Option<&NifMaterial>, images: &Assets<Image>) -> Diffuse {
    match material.map(|m| &m.base.base_color_texture) {
        // Not in the store yet (a model's materials go in with it).
        None => Diffuse::Loading,
        Some(None) => Diffuse::Missing,
        Some(Some(h)) if images.contains(h.id()) => Diffuse::Ready,
        Some(Some(_)) => Diffuse::Loading,
    }
}

/// Where a game terrain chunk's textures stand, its model prepared (#3096, module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Surface {
    /// Every shape it draws has its diffuse.
    Ready,
    /// A shape's diffuse is on its way: not spawned yet, the old chunks stay drawn.
    Pending,
    /// Nothing to draw: no textured shape, or every one cut out by the loaded cells.
    Empty,
    /// A shape it draws has no diffuse: drawn with the fallback colour (why).
    Failed(String),
}

/// The surface of a chunk whose drawable shapes are `shapes`. `cut_away`: the loaded cells
/// cut every triangle of the shape out, so it draws nothing whatever its textures (asked
/// only of shapes not ready).
fn surface<S>(shapes: &[S], diffuse: impl Fn(&S) -> Diffuse, cut_away: impl Fn(&S) -> bool) -> Surface {
    let mut drawn = false;
    let mut loading = false;
    let mut missing = false;
    for s in shapes {
        let d = diffuse(s);
        if d != Diffuse::Ready && cut_away(s) {
            continue;
        }
        drawn = true;
        loading |= d == Diffuse::Loading;
        missing |= d == Diffuse::Missing;
    }
    if loading {
        Surface::Pending
    } else if missing {
        Surface::Failed("a shape it draws has no diffuse texture (none named, or it failed to load)".into())
    } else if drawn {
        Surface::Ready
    } else {
        Surface::Empty
    }
}

/// [`surface`] of a game chunk's prepared model, drawn with the fully drawn cells `trim`
/// cut out (`None`: whole).
fn chunk_surface(model: &PreparedModel, materials: &Assets<NifMaterial>, images: &Assets<Image>, trim: Option<&HashSet<Grid>>) -> Surface {
    let shapes: Vec<&PreparedShape> = model.shapes.iter().filter(|s| s.cpu.as_deref().is_some_and(drawable)).collect();
    surface(
        &shapes,
        |s| diffuse_of(materials.get(&s.material), images),
        |s| {
            let (Some(full), Some(m)) = (trim, s.cpu.as_deref()) else { return false };
            terrain_mesh(m, Some((&(model.root * s.transform), full)), 0.0).is_none()
        },
    )
}

/// Whether a chunk with this surface is spawned now (and so counts as covering the chunks
/// it replaces): all but one whose diffuse is still on its way (#3096).
fn admit(surface: &Surface) -> bool {
    *surface != Surface::Pending
}

/// Whether the chosen chunks over outgoing chunk `o` are all spawned (`spawned`): only
/// then does it go.
fn covered(o: Chunk, chosen: &[Chunk], spawned: impl Fn(&Chunk) -> bool) -> bool {
    chosen.iter().filter(|&&ch| chunks_overlap(ch, o)).all(spawned)
}

/// The material a terrain shape whose diffuse is missing is drawn with (#3096): its own,
/// with the generated LOD's colour for a missing base texture (`lodgen::NO_BASE_SRGB`, as
/// terrain.rs draws one) as its base colour and no base texture, instead of white.
fn fallback_material(m: &NifMaterial) -> NifMaterial {
    let mut f = m.clone();
    let [r, g, b] = lodgen::NO_BASE_SRGB;
    f.base.base_color = Color::srgba(r, g, b, m.base.base_color.alpha());
    f.base.base_color_texture = None;
    f
}

/// A new fallback copy of material `src` ([`fallback_material`]) for one spawned shape:
/// its handle is the copy's only owner, so the copy (and the textures it holds) goes with
/// the entity, and nothing outlives the source material's own eviction.
fn fallback_for(materials: &mut Assets<NifMaterial>, src: &Handle<NifMaterial>) -> Option<Handle<NifMaterial>> {
    let f = materials.get(src).map(fallback_material)?;
    Some(materials.add(f))
}

/// A material's base colour (sRGB) and base texture, for the log.
fn describe_base(m: &StandardMaterial) -> String {
    let c = m.base_color.to_srgba();
    let tex = if m.base_color_texture.is_some() { "a base texture" } else { "no base texture" };
    format!("base colour sRGB {:.2} {:.2} {:.2}, {tex}", c.red, c.green, c.blue)
}

/// "<path>: <error>" for a diffuse a model named and could not load (`path`, resolved as
/// the texture cache resolves it): the error the cache reported for it, if any.
fn texture_failure(failures: &[String], path: &str) -> String {
    let prefix = format!("{}: ", path.to_ascii_lowercase());
    failures.iter().rev().find(|f| f.to_ascii_lowercase().starts_with(&prefix)).cloned().unwrap_or_else(|| format!("{path}: not loaded"))
}

/// The masks of the level-4 squares holding `cells` (cell, fully drawn; on the grid from
/// `o`): cells fully drawn, cells drawn at all (bit `y * 4 + x`, [`chunk_bit`]).
pub(crate) fn block_masks(cells: &[(Grid, bool)], o: Grid) -> HashMap<Grid, (u16, u16)> {
    let mut masks: HashMap<Grid, (u16, u16)> = HashMap::new();
    for &(c, full) in cells {
        let (ch, bit) = chunk_bit(c, o);
        let m = masks.entry(ch).or_default();
        m.1 |= bit;
        if full {
            m.0 |= bit;
        }
    }
    masks
}

/// Distant LOD each frame. The cells it gives way to are the loaded ones and, with the
/// full-detail ring on, the ring's (`crate::full_detail::DrawnCells`): both cut the LOD
/// terrain out and hide their object-block segments alike.
#[allow(clippy::too_many_arguments)]
pub fn update_lod(
    mut commands: Commands,
    drawn: crate::full_detail::DrawnCells,
    mut lod: ResMut<LodState>,
    root: Query<Entity, With<WorldRoot>>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut models: ModelCtx,
    pace: Res<crate::stream_pace::StreamPace>,
    mut contents: ResMut<BlockContents>,
) {
    if !lod.enabled {
        return;
    }
    contents.follow(&lod.world_name, lod.origin);
    let (Ok(root), Ok(cam)) = (root.single(), camera.single()) else { return };
    let update_started = std::time::Instant::now();
    let g = bevy_to_game(cam.translation());
    let c = cell_of(g.x, g.y);
    // Square indices on the world's LOD grid (`origin`).
    let o = lod.origin;
    let centre_chunk = ((c.0 - o.0).div_euclid(LEVEL), (c.1 - o.1).div_euclid(LEVEL));
    poll_generated(&mut lod, &mut models);
    let gen_radius = lod.gen.as_ref().map_or(-1, |g| g.radius);
    let near_gen = move |&(x, y): &Grid| ((x - o.0).div_euclid(LEVEL) - centre_chunk.0).abs().max(((y - o.1).div_euclid(LEVEL) - centre_chunk.1).abs()) <= gen_radius;
    if lod.centre != Some(c) {
        lod.centre = Some(c);
        let p = g.truncate();
        let s = lod.settings;
        lod.chosen = select_chunks(p, &s, o);
        lod.choice += 1;
        lod.squares = block_squares(p, &s, o);
        let finest: Vec<Grid> = lod.chosen.iter().filter(|ch| ch.0 == LEVEL).map(|ch| (ch.1, ch.2)).filter(near_gen).collect();
        schedule_generated(&mut lod, centre_chunk, &finest);
        let chosen: HashSet<Chunk> = lod.chosen.iter().copied().collect();
        let squares: HashSet<Grid> = lod.squares.iter().copied().collect();
        lod.meshes.retain(|ch, _| chosen.contains(ch));
        lod.pieces.retain(|sq, _| squares.contains(sq));
        lod.put_off_runs.retain(|sq, _| squares.contains(sq));
        let gone: Vec<Chunk> = lod.terrain.keys().filter(|ch| !chosen.contains(ch)).copied().collect();
        for ch in gone {
            if let Some(t) = lod.terrain.remove(&ch) {
                lod.outgoing.insert(ch, t);
            }
        }
        // Outgoing chunks chosen again are drawn as they were (their key is checked below).
        let back: Vec<Chunk> = lod.outgoing.keys().filter(|ch| chosen.contains(ch)).copied().collect();
        for ch in back {
            if let Some(t) = lod.outgoing.remove(&ch) {
                lod.terrain.insert(ch, t);
            }
        }
        let gone: Vec<Grid> = lod.block_entities.keys().filter(|sq| !squares.contains(sq)).copied().collect();
        for sq in gone {
            if let Some(b) = lod.block_entities.remove(&sq) {
                despawn_all(&mut commands, b.entities.into_iter().chain(b.pieces.into_iter().map(|(_, e)| e)));
            }
        }
    }
    // Which cells are drawn (loaded, or the full-detail ring's), and which are fully
    // drawn, per level-4 square.
    let full: HashSet<Grid> = drawn.full();
    let masks = block_masks(&drawn.cells(true), o);
    let mut changed = false;
    let started = std::time::Instant::now();
    let mut rebuilt = 0usize;
    // Above 60 fps a frame gets the same share of itself (stream_pace.rs).
    let budget_ms = pace.time(std::time::Duration::from_secs_f32(REBUILD_BUDGET_MS / 1000.0)).as_secs_f32() * 1000.0;
    let over_budget = |rebuilt: usize| rebuilt > 0 && started.elapsed().as_secs_f32() * 1000.0 >= budget_ms;
    let chosen = std::mem::take(&mut lod.chosen);
    for &chunk in &chosen {
        let mut cut: Vec<Grid> = full.iter().filter(|&&c| in_chunk(c, chunk)).copied().collect();
        cut.sort_unstable();
        let generated = chunk.0 == LEVEL
            && near_gen(&(chunk.1, chunk.2))
            && lod.gen.as_ref().is_some_and(|g| g.ready.contains_key(&(chunk.1, chunk.2)));
        let key = TerrainKey { full: cut, generated };
        if lod.terrain.get(&chunk).is_some_and(|t| t.key == key) {
            continue;
        }
        // The game's chunk model is prepared on a worker thread, as object blocks' are
        // (below), and its diffuse textures must be in the image store (#3096); the old
        // chunk stays drawn until they are. A missing diffuse is drawn with the fallback
        // colour (spawn_terrain).
        if !generated {
            let tp = lod.terrain_path(chunk);
            if models.data.vfs.exists(&tp) {
                let Some(model) = models.model_with_cpu_ready(&tp) else {
                    lod.block_stats.terrain_waited += 1;
                    continue;
                };
                // A model that did not load draws nothing, as before.
                let trim = (!key.full.is_empty()).then_some(&full);
                let surface = model.as_deref().map_or(Surface::Empty, |m| chunk_surface(m, &models.materials, &models.images, trim));
                if !admit(&surface) {
                    lod.surfaces.texture_waits += 1;
                    continue;
                }
            }
        }
        if over_budget(rebuilt) {
            break;
        }
        rebuilt += 1;
        if let Some(old) = lod.terrain.remove(&chunk) {
            despawn_all(&mut commands, old.entities);
        }
        let spawned = spawn_terrain(&mut commands, root, &mut lod, &mut models, chunk, key, &full);
        lod.terrain.insert(chunk, spawned);
        lod.terrain_rebuilds += 1;
        changed = true;
    }
    // Drop the outgoing chunks whose area the chosen chunks now cover.
    if !lod.outgoing.is_empty() {
        let terrain = &lod.terrain;
        let done: Vec<Chunk> = lod.outgoing.keys().copied().filter(|&o| covered(o, &chosen, |ch| terrain.contains_key(ch))).collect();
        for ch in done {
            if let Some(t) = lod.outgoing.remove(&ch) {
                despawn_all(&mut commands, t.entities);
            }
        }
        let kept = lod.outgoing.len();
        let st = &mut lod.block_stats;
        st.outgoing_frames += 1;
        st.outgoing_most = st.outgoing_most.max(kept);
    }
    lod.chosen = chosen;
    let squares = std::mem::take(&mut lod.squares);
    let blocks_started = std::time::Instant::now();
    let mut blocks_this_frame = 0usize;
    let mut waiting = 0usize;
    for &sq in &squares {
        let (full_mask, present) = masks.get(&sq).copied().unwrap_or_default();
        let key = BlockKey { full: full_mask, present };
        let steps: [u8; CELLS] = match present {
            0 => [0; CELLS],
            _ => std::array::from_fn(|i| fade::step(drawn.block_level(chunk_cell(sq, i)))),
        };
        match lod.block_entities.get_mut(&sq) {
            Some(b) if b.key == key => {
                // The same cells: only the per-cell parts' fade steps move.
                for &(i, e) in &b.pieces {
                    if b.steps[i] != steps[i] {
                        commands.entity(e).insert(fade::complement(steps[i]));
                    }
                }
                b.steps = steps;
                if !lod.put_off_runs.is_empty() {
                    lod.put_off_runs.remove(&sq);
                }
            }
            b => {
                let had = b.is_some();
                // The block's model is prepared on a worker thread: until it is, a changed
                // block stays as it is and a new square stays empty (tried again next
                // frame, as nothing marks it done). Loading it here took up to a second
                // for a row of squares coming into range (#1038), and one cold block
                // could still overrun the budget below (#1048).
                let bp = lod.block_path(sq.0, sq.1);
                // Over the budget: only ask for the model (taking a finished one in costs
                // main-thread time too), and wait for a later frame. New squares used to
                // skip the budget (#1038); waiting costs a little more pop-in at the
                // view's edge, nearest squares first (`squares` is sorted). The first
                // block of the frame goes through whatever terrain spent above (#1058).
                if blocks_this_frame > 0 && over_budget(rebuilt) {
                    if models.data.vfs.exists(&bp) {
                        models.request_model_with_cpu(&bp);
                    }
                    let run = lod.put_off_runs.entry(sq).or_default();
                    *run += 1;
                    let run = *run;
                    let st = &mut lod.block_stats;
                    st.put_off += 1;
                    st.longest_put_off = st.longest_put_off.max(run);
                    waiting += 1;
                    continue;
                }
                let model = match models.data.vfs.exists(&bp).then(|| models.model_with_cpu_ready(&bp)) {
                    None => None,
                    Some(Some(m)) => m,
                    Some(None) => {
                        lod.block_stats.waited += 1;
                        waiting += 1;
                        continue;
                    }
                };
                lod.put_off_runs.remove(&sq);
                // What the block holds by cell, for the distant objects (#3014).
                contents.set(sq, model.as_ref().map_or(0, |m| segment_mask(m.shapes.iter().map(|s| s.segments.as_slice()))));
                if had {
                    lod.block_stats.rebuilt += 1;
                } else {
                    lod.block_stats.spawned += 1;
                }
                rebuilt += 1;
                blocks_this_frame += 1;
                if let Some(old) = lod.block_entities.remove(&sq) {
                    despawn_all(&mut commands, old.entities.into_iter().chain(old.pieces.into_iter().map(|(_, e)| e)));
                }
                let spawned = spawn_block(&mut commands, root, &mut lod, &mut models, model, sq, key, steps);
                lod.block_entities.insert(sq, spawned);
                changed = true;
            }
        }
    }
    lod.squares = squares;
    if rebuilt > 0 {
        pace.spent(started.elapsed());
    }
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f32() * 1000.0;
    let (blocks_ms, update_ms) = (ms(blocks_started), ms(update_started));
    let st = &mut lod.block_stats;
    st.most_in_frame = st.most_in_frame.max(blocks_this_frame);
    st.waiting_now = waiting;
    st.blocks_slowest_ms = st.blocks_slowest_ms.max(blocks_ms);
    st.update_slowest_ms = st.update_slowest_ms.max(update_ms);
    st.slow_updates += usize::from(update_ms > SLOW_UPDATE_MS);
    if changed {
        let game = |t: &&TerrainEntities| t.drawn == Some(false);
        lod.chunks = lod.terrain.values().filter(game).count();
        lod.levels = std::array::from_fn(|i| lod.terrain.iter().filter(|(ch, t)| ch.0 == LEVELS[i] && t.drawn.is_some()).count());
        lod.generated = lod.terrain.values().filter(|t| t.drawn == Some(true)).count();
        lod.blocks = lod.block_entities.values().filter(|b| b.drawn).count();
    }
}

fn spawn_entity(commands: &mut Commands, root: Entity, mesh: Handle<Mesh>, material: Handle<NifMaterial>, t: Transform, fade: Option<VisibilityRange>) -> Entity {
    let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), t)).id();
    if let Some(r) = fade {
        commands.entity(e).insert(r);
    }
    commands.entity(root).add_child(e);
    e
}

/// Spawn one terrain chunk: generated when ready (level 4), else the game's.
fn spawn_terrain(
    commands: &mut Commands,
    root: Entity,
    lod: &mut LodState,
    models: &mut ModelCtx,
    chunk: Chunk,
    key: TerrainKey,
    full: &HashSet<Grid>,
) -> TerrainEntities {
    let (level, cx, cy) = chunk;
    let overlaps = !key.full.is_empty();
    let mut out = TerrainEntities { entities: Vec::new(), key, drawn: None };
    let generated = lod.gen.as_ref().filter(|_| out.key.generated);
    if let Some((gen, gc)) = generated.and_then(|g| g.ready.get(&(cx, cy)).map(|c| (g, c))) {
        let mesh = if overlaps {
            // Cell-aligned, so loaded cells are cut out exactly.
            let keep: Vec<bool> = (0..CELLS).map(|i| !out.key.full.contains(&chunk_cell((cx, cy), i))).collect();
            let hs: Vec<Option<&[f32]>> = gc.heights.iter().map(|h| h.as_deref()).collect();
            lodgen::chunk_mesh(&hs, &keep, gen.job.params.step).map(|m| models.meshes.add(m))
        } else {
            gc.full.clone()
        };
        if let Some(m) = mesh {
            let t = Transform::from_translation(Vec3::new(cx as f32 * CELL_SIZE, cy as f32 * CELL_SIZE, 0.0));
            out.entities.push(spawn_entity(commands, root, m, gc.material.clone(), t, None));
        }
        out.drawn = Some(true);
        return out;
    }
    let tp = lod.terrain_path(chunk);
    // Chunks exist only inside the worldspace's LOD grid; skip the rest quietly.
    let Some(model) = models.data.vfs.exists(&tp).then(|| models.model_with_cpu(&tp)).flatten() else { return out };
    let skirt = skirt_depth(level);
    if !overlaps && !lod.meshes.contains_key(&chunk) {
        // The GPU mesh cannot be read back once uploaded; derive from the CPU copy.
        let fixed = model
            .shapes
            .iter()
            .map(|s| s.cpu.as_deref().and_then(|m| terrain_mesh(m, None, skirt)).map(|m| models.meshes.add(m)))
            .collect();
        lod.meshes.insert(chunk, fixed);
    }
    for (i, s) in model.shapes.iter().enumerate() {
        let st = model.root * s.transform;
        let mesh = if overlaps {
            let Some(m) = s.cpu.as_deref() else { continue };
            let Some(t) = terrain_mesh(m, Some((&st, full)), skirt) else { continue };
            models.meshes.add(t)
        } else {
            let Some(Some(m)) = lod.meshes[&chunk].get(i) else { continue };
            m.clone()
        };
        // Push LOD terrain behind loaded terrain where both exist. Its normal map is
        // in model space (green = north, module docs), so it must not get the
        // green flip that tangent-space NIF normal maps need.
        if models.materials.get(&s.material).is_some_and(|m| m.base.depth_bias == 0.0 || m.base.flip_normal_map_y) {
            if let Some(mut mat) = models.materials.get_mut(&s.material) {
                mat.base.depth_bias = -50.0;
                mat.base.flip_normal_map_y = false;
            }
        }
        // A missing diffuse would draw white: the fallback colour instead (#3096). The gate
        // in `update_lod` leaves none still on its way; one would be counted (0 expected).
        let material = match diffuse_of(models.materials.get(&s.material), &models.images) {
            Diffuse::Ready => s.material.clone(),
            Diffuse::Missing => {
                let Some(h) = fallback_for(&mut models.materials, &s.material) else { continue };
                lod.surfaces.fallback_shapes += 1;
                if lod.warned.insert(chunk) {
                    lod.surfaces.fallback_chunks += 1;
                    // What the entity is bound to, read back from the store.
                    let bound = models.materials.get(&h).map_or_else(|| "not in the store".to_string(), |m| describe_base(&m.base));
                    let missing = s.missing_diffuse.as_deref().map_or_else(
                        || "no diffuse named".to_string(),
                        |raw| texture_failure(&models.textures.failures, &nvf_assets::texture::resolved_path(raw, |p| models.data.vfs.exists(p))),
                    );
                    warn!("distant LOD: terrain chunk level {level} x{cx} y{cy} drawn with the fallback colour (shape {i} bound to {bound}): {missing}");
                }
                h
            }
            Diffuse::Loading => {
                lod.surfaces.drawn_without_diffuse += 1;
                if lod.unguarded.insert(chunk) {
                    warn!("distant LOD: terrain chunk level {level} x{cx} y{cy} drawn without its diffuse texture (shape {i}, still loading)");
                }
                s.material.clone()
            }
        };
        out.entities.push(spawn_entity(commands, root, mesh, material, Transform::from_matrix(Mat4::from(st)), None));
    }
    out.drawn = Some(false);
    out
}

/// Spawn one square's object block: whole, or by cell where cells are loaded. Every part
/// keeps a crossfade range (fully drawn at rest), so switching between the two needs no
/// other pipeline.
#[allow(clippy::too_many_arguments)]
fn spawn_block(
    commands: &mut Commands,
    root: Entity,
    lod: &mut LodState,
    models: &mut ModelCtx,
    model: Option<Arc<PreparedModel>>,
    (cx, cy): Grid,
    key: BlockKey,
    steps: [u8; CELLS],
) -> BlockEntities {
    let mut out = BlockEntities { key, steps, ..default() };
    let Some(model) = model else { return out };
    if key.present == 0 {
        for s in &model.shapes {
            let t = Transform::from_matrix(Mat4::from(model.root * s.transform));
            out.entities.push(spawn_entity(commands, root, s.mesh.clone(), s.material.clone(), t, Some(fade::complement(0))));
        }
        out.drawn = !model.shapes.is_empty();
        return out;
    }
    let pieces = lod.pieces.entry((cx, cy)).or_insert_with(|| split_block(&model, (cx, cy), &mut models.meshes));
    for (i, parts) in pieces.iter().enumerate() {
        if key.full & (1 << i) != 0 {
            continue;
        }
        for (mesh, material, t) in parts {
            let e = spawn_entity(commands, root, mesh.clone(), material.clone(), *t, Some(fade::complement(steps[i])));
            out.pieces.push((i, e));
        }
    }
    out.drawn = !out.pieces.is_empty();
    out
}

/// A square's object block split by cell: by the block's own segments (module docs),
/// which keep every object whole. A shape without usable segments is split by triangle
/// instead, each going to the cell holding its centre (triangles reaching past the
/// square go to the nearest edge cell).
fn split_block(model: &PreparedModel, chunk: Grid, meshes: &mut Assets<Mesh>) -> Pieces {
    let mut out: Pieces = vec![Vec::new(); CELLS];
    for s in &model.shapes {
        let Some(m) = s.cpu.as_deref() else { continue };
        let st = model.root * s.transform;
        let t = Transform::from_matrix(Mat4::from(st));
        let parts = split_by_segment(m, &s.segments).unwrap_or_else(|| split_by_cell(m, &st, chunk));
        for (i, part) in parts.into_iter().enumerate() {
            if let Some(p) = part {
                out[i].push((meshes.add(p), s.material.clone(), t));
            }
        }
    }
    out
}

/// The cell of a level-4 square (as an index into its masks) that segment `i` of its
/// object block holds: cell (i / 4, i mod 4), x first (module docs).
fn segment_cell(i: usize) -> usize {
    let l = LEVEL as usize;
    (i % l) * l + i / l
}

/// A triangle-list mesh split by its shape's segments (`segments`: triangle ranges, one
/// per cell in [`segment_cell`] order), each part keeping only the vertices it uses.
/// `None` when the shape has no segments, more of them than the square has cells, or
/// they reach past the mesh's triangles.
fn split_by_segment(mesh: &Mesh, segments: &[std::ops::Range<usize>]) -> Option<Vec<Option<Mesh>>> {
    if segments.is_empty() || segments.len() > CELLS {
        return None;
    }
    let idx: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|&v| v as u32).collect(),
    };
    let mut out: Vec<Option<Mesh>> = vec![None; CELLS];
    for (i, r) in segments.iter().enumerate() {
        let tris = idx.get(3 * r.start..3 * r.end)?;
        if !tris.is_empty() {
            out[segment_cell(i)] = Some(submesh(mesh, tris));
        }
    }
    Some(out)
}

/// A triangle-list mesh split by the cells of `chunk` its triangles' centres fall in
/// (`to_world`: mesh to game space), each part keeping only the vertices it uses.
fn split_by_cell(mesh: &Mesh, to_world: &Affine3A, (cx, cy): Grid) -> Vec<Option<Mesh>> {
    let mut tris: Vec<Vec<u32>> = vec![Vec::new(); CELLS];
    if let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        let idx: Vec<u32> = match mesh.indices() {
            Some(Indices::U32(i)) => i.clone(),
            Some(Indices::U16(i)) => i.iter().map(|&v| v as u32).collect(),
            None => (0..pos.len() as u32).collect(),
        };
        for t in idx.as_chunks::<3>().0 {
            let c = t.iter().map(|&v| to_world.transform_point3(Vec3::from_array(pos[v as usize]))).sum::<Vec3>() / 3.0;
            let x = ((c.x / CELL_SIZE).floor() as i32 - cx).clamp(0, LEVEL - 1);
            let y = ((c.y / CELL_SIZE).floor() as i32 - cy).clamp(0, LEVEL - 1);
            tris[(y * LEVEL + x) as usize].extend_from_slice(t);
        }
    }
    tris.into_iter().map(|t| (!t.is_empty()).then(|| submesh(mesh, &t))).collect()
}

/// The part of `mesh` drawn by the triangles `tris`, with only the vertices they use.
fn submesh(mesh: &Mesh, tris: &[u32]) -> Mesh {
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut used: Vec<usize> = Vec::new();
    let idx: Vec<u32> = tris
        .iter()
        .map(|&v| {
            *remap.entry(v).or_insert_with(|| {
                used.push(v as usize);
                used.len() as u32 - 1
            })
        })
        .collect();
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    for (id, values) in mesh.attributes() {
        out.insert_attribute(*id, gather(values, &used));
    }
    out.insert_indices(Indices::U32(idx));
    out
}

/// The values of the vertices `used`, in that order.
fn gather(values: &VertexAttributeValues, used: &[usize]) -> VertexAttributeValues {
    use VertexAttributeValues as V;
    macro_rules! pick {
        ($($v:ident),*) => {
            match values {
                $(V::$v(a) => V::$v(used.iter().map(|&i| a[i]).collect()),)*
            }
        };
    }
    pick!(
        Uint8, Uint8x2, Uint8x4, Sint8, Sint8x2, Sint8x4, Unorm8, Unorm8x2, Unorm8x4, Snorm8, Snorm8x2, Snorm8x4, Uint16,
        Uint16x2, Uint16x4, Sint16, Sint16x2, Sint16x4, Unorm16, Unorm16x2, Unorm16x4, Snorm16, Snorm16x2, Snorm16x4,
        Float16, Float16x2, Float16x4, Float32, Float32x2, Float32x3, Float32x4, Uint32, Uint32x2, Uint32x3, Uint32x4,
        Sint32, Sint32x2, Sint32x3, Sint32x4, Float64, Float64x2, Float64x3, Float64x4, Unorm8x4Bgra,
        Unorm10_10_10_2
    )
}

/// Queue bakes for the generated chunks `want` (level-4 squares chosen near the camera),
/// nearest first; forget waiting ones no longer wanted (but near) and ready ones far behind.
fn schedule_generated(lod: &mut LodState, centre_chunk: Grid, want: &[Grid]) {
    let o = lod.origin;
    let Some(gen) = lod.gen.as_mut() else { return };
    let gr = gen.radius;
    let index = move |(x, y): Grid| ((x - o.0).div_euclid(LEVEL), (y - o.1).div_euclid(LEVEL));
    let dist = |&ch: &Grid| {
        let (x, y) = index(ch);
        (x - centre_chunk.0).abs().max((y - centre_chunk.1).abs())
    };
    gen.ready.retain(|ch, _| dist(ch) <= gr + 2);
    let mut order: Vec<(i32, Grid)> = want
        .iter()
        .map(|&ch| {
            let (x, y) = index(ch);
            let (dx, dy) = (x - centre_chunk.0, y - centre_chunk.1);
            (dx * dx + dy * dy, ch)
        })
        .collect();
    order.sort();
    let mut queue: Vec<Grid> = order.into_iter().map(|(_, ch)| ch).filter(|ch| !gen.ready.contains_key(ch) && !gen.skip.contains(ch)).collect();
    // Bakes of the camera's and neighbouring squares stay queued, last, while unchosen.
    let wanted: HashSet<Grid> = queue.iter().copied().collect();
    let mut near: Vec<Grid> = gen.pending.iter().copied().filter(|ch| !wanted.contains(ch) && dist(ch) <= 1).collect();
    near.sort_unstable();
    queue.extend(near);
    gen.baker.schedule(&queue, &mut gen.pending);
}

/// Collect finished bakes, a few a frame: upload their textures and build their full
/// meshes (`update_lod` redraws their chunks, whose keys change).
fn poll_generated(lod: &mut LodState, models: &mut ModelCtx) {
    let Some(gen) = lod.gen.as_mut() else { return };
    let done: Vec<_> = std::iter::from_fn(|| gen.baker.try_recv()).take(BAKES_PER_FRAME).collect();
    for (chunk, _) in &done {
        gen.pending.remove(chunk);
    }
    let bc = models.formats.as_deref().is_none_or(|f| f.0.contains(CompressedImageFormats::BC));
    for (chunk, result) in done {
        let baked = match result {
            Ok(Some(b)) => b,
            Ok(None) => {
                gen.skip.insert(chunk);
                continue;
            }
            Err(e) => {
                warn!("generated LOD x{} y{}: {e}", chunk.0, chunk.1);
                gen.skip.insert(chunk);
                continue;
            }
        };
        let image = |bytes: &[u8], srgb: bool| {
            dds_image(bytes, srgb, bc).map(|mut img| {
                // One image per chunk: clamp so its edges do not bleed from the far side.
                img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::ClampToEdge,
                    address_mode_v: ImageAddressMode::ClampToEdge,
                    mag_filter: ImageFilterMode::Linear,
                    min_filter: ImageFilterMode::Linear,
                    mipmap_filter: ImageFilterMode::Linear,
                    anisotropy_clamp: 8,
                    ..ImageSamplerDescriptor::linear()
                });
                img.texture_descriptor.label = Some("generated lod");
                img
            })
        };
        let (Ok(d), Ok(n)) = (image(&baked.diffuse, true), image(&baked.normals, false)) else {
            warn!("generated LOD x{} y{}: cannot upload its textures", chunk.0, chunk.1);
            gen.skip.insert(chunk);
            continue;
        };
        let material = models.materials.add(nif_material(StandardMaterial {
            base_color_texture: Some(models.images.add(d)),
            normal_map_texture: Some(models.images.add(n)),
            // As the loaded terrain's materials (terrain.rs).
            perceptual_roughness: 0.95,
            reflectance: 0.2,
            ..default()
        }));
        let hs: Vec<Option<&[f32]>> = baked.heights.iter().map(|h| h.as_deref()).collect();
        let full = lodgen::chunk_mesh(&hs, &[true; 16], gen.job.params.step).map(|m| models.meshes.add(m));
        if baked.baked {
            gen.baked += 1;
        } else {
            gen.cached += 1;
        }
        gen.ready.insert(chunk, GenChunk { heights: baked.heights, material, full });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(lo: Grid, hi: Grid) -> HashSet<Grid> {
        (lo.0..=hi.0).flat_map(|x| (lo.1..=hi.1).map(move |y| (x, y))).collect()
    }

    #[test]
    fn inside_test() {
        let full = square((-2, -2), (2, 2));
        assert!(inside(Vec3::new(0.0, 0.0, 0.0), &full));
        assert!(inside(Vec3::new(3.0 * CELL_SIZE - 1.0, -2.0 * CELL_SIZE + 1.0, 0.0), &full));
        assert!(!inside(Vec3::new(3.0 * CELL_SIZE + 1.0, 0.0, 0.0), &full));
        // The outer edge is outside, inner edges between loaded cells inside.
        assert!(!inside(Vec3::new(3.0 * CELL_SIZE, 0.0, 0.0), &full));
        assert!(inside(Vec3::new(CELL_SIZE, CELL_SIZE, 0.0), &full));
        // A cell of the square still loading is not cut out, nor its edges.
        let mut partial = full.clone();
        partial.remove(&(2, 0));
        assert!(!inside(Vec3::new(2.5 * CELL_SIZE, 0.5 * CELL_SIZE, 0.0), &partial));
        assert!(!inside(Vec3::new(2.0 * CELL_SIZE, 0.5 * CELL_SIZE, 0.0), &partial));
    }

    #[test]
    fn chunk_cells() {
        assert_eq!(chunk_bit((5, -1), (0, 0)), ((4, -4), 1 << (3 * 4 + 1)));
        // Freeside's grid starts at (-10, -8): cell (-3, -6) is in square (-6, -8).
        assert_eq!(chunk_bit((-3, -6), (-10, -8)), ((-6, -8), 1 << (2 * 4 + 3)));
        assert_eq!(chunk_cell((4, -4), 13), (5, -1));
        assert!(in_chunk((5, -1), (4, 4, -4)));
        assert!(in_chunk((-32, 31), (32, -32, 0)));
        assert!(!in_chunk((0, 31), (32, -32, 0)));
    }

    #[test]
    fn distance_to_squares() {
        let p = Vec2::new(100.0, 200.0);
        assert_eq!(square_distance(p, (0, 0), 1), 0.0);
        assert_eq!(square_distance(p, (1, 0), 1), CELL_SIZE - 100.0);
        assert_eq!(square_distance(p, (-2, 0), 1), CELL_SIZE + 100.0);
        let d = square_distance(p, (1, 1), 4);
        assert!((d - ((CELL_SIZE - 100.0).powi(2) + (CELL_SIZE - 200.0).powi(2)).sqrt()).abs() < 1e-2);
    }

    #[test]
    fn chunks_overlap_by_cells() {
        // A level-8 chunk holds the four level-4 chunks of its area, and touches no other.
        assert!(chunks_overlap((8, 0, 0), (4, 4, 4)));
        assert!(chunks_overlap((4, 4, 4), (8, 0, 0)));
        assert!(!chunks_overlap((8, 0, 0), (4, 8, 0)));
        assert!(!chunks_overlap((4, -4, 0), (4, 0, 0)));
        assert!(chunks_overlap((32, -32, -32), (4, -4, -4)));
    }

    /// The chosen chunks tile the ground without overlaps, use level 4 within `near` and
    /// reach no farther than the terrain's reach (250,000 at the defaults, past the object
    /// blocks' 125,000). Levels 4, 8 and 16 appear everywhere: each level's band of
    /// distances (50,000..98,304 for 8, 98,304..196,608 for 16) is wider than its chunk
    /// side (32,768, 65,536), so the chunks in the camera's own row hit it.
    #[test]
    fn chunk_choice() {
        let s = LodSettings::default();
        let reach = s.terrain_reach();
        assert_eq!(reach, 250_000.0);
        for p in [Vec2::new(-69_500.0, 2_500.0), Vec2::new(0.0, 0.0), Vec2::new(55_500.0, 37_536.0), Vec2::new(-1.0, -1.0)] {
            let chosen = select_chunks(p, &s, (0, 0));
            let mut cells: HashMap<Grid, Chunk> = HashMap::new();
            for &ch in &chosen {
                let (l, x, y) = ch;
                assert!(LEVELS.contains(&l));
                assert_eq!((x.rem_euclid(l), y.rem_euclid(l)), (0, 0), "{ch:?} aligned");
                assert!(square_distance(p, (x, y), l) < reach);
                for cx in x..x + l {
                    for cy in y..y + l {
                        assert!(cells.insert((cx, cy), ch).is_none(), "cell ({cx}, {cy}) twice");
                    }
                }
            }
            // Every cell nearer than the reach is covered; those nearer than `near` at level 4.
            let r = (reach / CELL_SIZE) as i32 + 2;
            let (cx, cy) = cell_of(p.x, p.y);
            for x in cx - r..=cx + r {
                for y in cy - r..=cy + r {
                    let d = square_distance(p, (x, y), 1);
                    if d < s.near {
                        assert_eq!(cells.get(&(x, y)).map(|c| c.0), Some(4), "cell ({x}, {y}) at {d}");
                    }
                    // Up to rounding at the chunk scale: a cell inside the reach lies in a
                    // chunk whose nearest point is no farther.
                    if d < reach {
                        assert!(cells.contains_key(&(x, y)), "cell ({x}, {y}) at {d} not covered");
                    }
                }
            }
            for l in [4, 8, 16] {
                assert!(chosen.iter().any(|c| c.0 == l), "level {l} at {p}");
            }
            // Terrain is drawn past the object blocks' `far`.
            assert!(chosen.iter().any(|&(l, x, y)| square_distance(p, (x, y), l) >= s.far), "nothing beyond far at {p}");
            // Level 32 beyond its split distance when the reach goes far enough past it.
            assert!(select_chunks(p, &LodSettings { far: 400_000.0, ..s }, (0, 0)).iter().any(|c| c.0 == 32));
        }
        // Coarser chunks stay outside their split distance.
        let p = Vec2::new(0.0, 0.0);
        for (l, x, y) in select_chunks(p, &s, (0, 0)) {
            let d = square_distance(p, (x, y), l);
            assert!(l == 4 || d >= s.near.max(s.split * l as f32 * CELL_SIZE), "({l}, {x}, {y}) at {d}");
        }
        assert!(select_chunks(p, &LodSettings { far: 0.0, terrain_far: 0.0, ..s }, (0, 0)).is_empty());
    }

    /// From the hill above Goodsprings where the original was captured (#3055), on
    /// WastelandNV's grid (origin -64, -64): level-32 chunks appear. The level-32 columns
    /// lie at x distances 0, 65,725 and 196,797 to the west and 65,347 and 196,419 to the
    /// east, the rows at 0 (its own), 5,770 south and 125,302 north. So (32, -96, 0) is
    /// 196,797 away (past level 32's split distance 1.5 × 32 × 4,096 = 196,608, inside
    /// 250,000) and (32, 32, 32) √(196,419² + 125,302²) ≈ 232,983.
    #[test]
    fn level_32_from_the_goodsprings_hill() {
        let s = LodSettings::default();
        let p = Vec2::new(-65_347.0, 5_770.0);
        let chosen = select_chunks(p, &s, (-64, -64));
        assert!(chosen.contains(&(32, -96, 0)), "{chosen:?}");
        assert!(chosen.contains(&(32, 32, 32)), "{chosen:?}");
        let d = square_distance(p, (32, 32), 32);
        assert!((d - 232_983.0).abs() < 10.0, "{d}");
        // With the old single reach (the terrain stopping at `far`) neither is drawn.
        let old = select_chunks(p, &LodSettings { terrain_far: 0.0, ..s }, (-64, -64));
        assert!(!old.iter().any(|c| c.0 == 32));
        assert!(old.iter().all(|&(l, x, y)| square_distance(p, (x, y), l) < s.far));
    }

    /// The terrain's reach is `terrain_far`, or `far` when that is larger; a larger `far`
    /// draws terrain past 250,000.
    #[test]
    fn terrain_reach_follows_a_larger_far() {
        let s = LodSettings::default();
        assert_eq!(LodSettings { far: 300_000.0, ..s }.terrain_reach(), 300_000.0);
        assert_eq!(LodSettings { terrain_far: 100_000.0, ..s }.terrain_reach(), 125_000.0);
        assert_eq!(LodSettings { far: 0.0, ..s }.terrain_reach(), 250_000.0);
        // From (0, 0) the level-32 chunk two columns east is 262,144 away: beyond the
        // default reach, inside a `far` of 300,000.
        let p = Vec2::new(0.0, 0.0);
        assert_eq!(square_distance(p, (64, 0), 32), 262_144.0);
        assert!(!select_chunks(p, &s, (0, 0)).iter().any(|&(_, x, y)| x >= 64 || y >= 64));
        assert!(select_chunks(p, &LodSettings { far: 300_000.0, ..s }, (0, 0)).contains(&(32, 64, 0)));
        // Object blocks gone (`far` 0), the terrain stays.
        assert!(!select_chunks(p, &LodSettings { far: 0.0, ..s }, (0, 0)).is_empty());
        assert!(block_squares(p, &LodSettings { far: 0.0, ..s }, (0, 0)).is_empty());
    }

    /// Object blocks stop at `far` (125,000) though the terrain reaches 250,000: exactly
    /// the level-4 squares nearer than `far` are listed, nearest first.
    #[test]
    fn block_squares_stop_at_far() {
        let s = LodSettings::default();
        let o = (-64, -64);
        for p in [Vec2::new(-65_347.0, 5_770.0), Vec2::new(0.0, 0.0), Vec2::new(-1.0, -1.0)] {
            let squares = block_squares(p, &s, o);
            assert!(!squares.is_empty());
            let ds: Vec<f32> = squares.iter().map(|&sq| square_distance(p, sq, LEVEL)).collect();
            assert!(ds.windows(2).all(|w| w[0] <= w[1]), "nearest first at {p}");
            let set: HashSet<Grid> = squares.iter().copied().collect();
            assert_eq!(set.len(), squares.len());
            let (cx, cy) = cell_of(p.x, p.y);
            // 40 cells = 163,840 units: past `far` plus a square's side.
            for x in (align(cx - 40, o.0, LEVEL)..=cx + 40).step_by(LEVEL as usize) {
                for y in (align(cy - 40, o.1, LEVEL)..=cy + 40).step_by(LEVEL as usize) {
                    let d = square_distance(p, (x, y), LEVEL);
                    assert_eq!(set.contains(&(x, y)), d < s.far, "square ({x}, {y}) at {d}");
                }
            }
        }
        assert_eq!((squares_within(250_000.0), squares_within(125_000.0), squares_within(0.0), squares_within(-1.0)), (16, 8, 0, 0));
    }

    #[test]
    fn chunks_follow_the_worlds_lod_origin() {
        // Freeside: south-west cell (-10, -8) [observed: freesideworld.dlodsettings,
        // whose files are level4.x-10 … x6, y-8 … y12].
        let b = [4, 0, 0, 0, 8, 0, 0, 0, 32, 0, 0, 0, 0xF6, 0xFF, 0xF8, 0xFF, 7, 0, 12, 0, 4, 0, 0, 0];
        assert_eq!(settings_origin(&b), Some((-10, -8)));
        assert_eq!(settings_origin(&b[..13]), None);
        let s = LodSettings::default();
        let o = (-10, -8);
        let chosen = select_chunks(Vec2::new(-6500.0, -10500.0), &s, o);
        assert!(!chosen.is_empty());
        for &(l, x, y) in &chosen {
            assert_eq!(((x - o.0).rem_euclid(l), (y - o.1).rem_euclid(l)), (0, 0), "({l}, {x}, {y}) off the grid");
        }
        assert!(chosen.contains(&(4, -2, -4)), "the camera's own square, freesideworld.level4.x-2.y-4");
        assert_eq!((align(-3, -10, 4), align(-6, -8, 4)), (-6, -8));
        assert_eq!((align(-11, -10, 4), align(0, 0, 32)), (-14, 0));
    }

    #[test]
    fn split_by_cell_parts() {
        // Two triangles in cell (0, 0) sharing vertices, one in cell (2, 1) of chunk (0, 0).
        let s = CELL_SIZE;
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        let pos = vec![
            [1.0, 1.0, 0.0],
            [9.0, 1.0, 0.0],
            [9.0, 9.0, 0.0],
            [1.0, 9.0, 0.0],
            [2.2 * s, 1.2 * s, 0.0],
            [2.4 * s, 1.2 * s, 0.0],
            [2.3 * s, 1.4 * s, 0.0],
        ];
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, (0..7).map(|i| [i as f32, 0.0]).collect::<Vec<[f32; 2]>>());
        m.insert_indices(Indices::U16(vec![0, 1, 2, 0, 2, 3, 4, 5, 6]));
        let parts = split_by_cell(&m, &Affine3A::IDENTITY, (0, 0));
        assert_eq!(parts.len(), CELLS);
        assert_eq!(parts.iter().filter(|p| p.is_some()).count(), 2);
        let a = parts[0].as_ref().unwrap();
        assert_eq!(a.count_vertices(), 4);
        assert_eq!(a.indices().unwrap().len(), 6);
        let b = parts[LEVEL as usize + 2].as_ref().unwrap();
        assert_eq!(b.count_vertices(), 3);
        let Some(VertexAttributeValues::Float32x2(uv)) = b.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        assert_eq!(uv, &vec![[4.0, 0.0], [5.0, 0.0], [6.0, 0.0]]);
    }

    /// #3014: a block's mask sets the cells of its non-empty segments (segment i is cell
    /// (i / 4, i mod 4), x-major); a shape without segments, or with too many, holds every
    /// cell; no shape holds nothing; shapes add up. The contents answer per cell.
    #[test]
    fn block_contents_from_segments() {
        let segments = [0..0, 0..2, 2..2, 2..2, 2..2, 2..2, 2..3];
        assert_eq!(segment_mask([&segments[..]]), (1 << segment_cell(1)) | (1 << segment_cell(6)));
        assert_eq!(segment_mask([&segments[..]]), (1 << 4) | (1 << 9), "cells (0, 1) and (1, 2)");
        assert_eq!(segment_mask(std::iter::empty::<&[std::ops::Range<usize>]>()), 0, "a block with no shape");
        assert_eq!(segment_mask([&[][..]]), u16::MAX, "a shape without segments: every cell");
        assert_eq!(segment_mask([&vec![0..1; CELLS + 1][..]]), u16::MAX, "too many segments");
        let other = 0..1;
        assert_eq!(segment_mask([&segments[..], std::slice::from_ref(&other)]), (1 << 4) | (1 << 9) | 1, "two shapes");
        let mut c = BlockContents::default();
        c.follow("wastelandnv", (-64, -64));
        assert_eq!(c.segment((-20, 1)), None, "unknown until its block is prepared");
        // Square (-20, 0) on the grid from (-64, -64); cell (-20, 1) is its bit 4.
        c.set((-20, 0), 1 << 4);
        assert_eq!(c.segment((-20, 1)), Some(true));
        assert_eq!(c.segment((-19, 1)), Some(false), "an empty segment");
        assert_eq!(c.segment((-17, 3)), Some(false), "same square");
        assert_eq!(c.segment((-16, 0)), None, "the next square");
        c.follow("wastelandnv", (-64, -64));
        assert_eq!(c.known(), 1, "the same files: kept");
        c.follow("freeside", (-10, -8));
        assert_eq!((c.known(), c.segment((-20, 1))), (0, None), "another worldspace's files: forgotten");
    }

    #[test]
    fn split_by_segment_keeps_objects_whole() {
        // An object of two triangles in segment 1 (cell (0, 1)) whose second triangle lies
        // in cell (1, 1), and a triangle in segment 6 (cell (1, 2)); segment 0 is empty.
        let s = CELL_SIZE;
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        let pos = vec![
            [0.5 * s, 1.5 * s, 0.0],
            [0.9 * s, 1.5 * s, 0.0],
            [0.9 * s, 1.9 * s, 0.0],
            [1.4 * s, 1.5 * s, 0.0],
            [1.2 * s, 2.2 * s, 0.0],
            [1.4 * s, 2.2 * s, 0.0],
            [1.3 * s, 2.4 * s, 0.0],
        ];
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_indices(Indices::U32(vec![0, 1, 2, 1, 3, 2, 4, 5, 6]));
        let segments = [0..0, 0..2, 2..2, 2..2, 2..2, 2..2, 2..3];
        let parts = split_by_segment(&m, &segments).unwrap();
        assert_eq!(parts.len(), CELLS);
        assert_eq!(parts.iter().filter(|p| p.is_some()).count(), 2);
        // Cell (0, 1) is bit 1 * 4 + 0, cell (1, 2) bit 2 * 4 + 1 (`chunk_bit`).
        assert_eq!((segment_cell(1), segment_cell(6)), (4, 9));
        assert_eq!(chunk_bit((0, 1), (0, 0)).1, 1 << segment_cell(1));
        let object = parts[4].as_ref().unwrap();
        assert_eq!((object.count_vertices(), object.indices().unwrap().len()), (4, 6));
        assert_eq!(parts[9].as_ref().unwrap().count_vertices(), 3);
        // By triangle centre the object is cut in two.
        let cut = split_by_cell(&m, &Affine3A::IDENTITY, (0, 0));
        assert_eq!(cut[4].as_ref().unwrap().indices().unwrap().len(), 3);
        assert!(cut[5].is_some());
        // No segments, too many, or ranges past the mesh: not split this way.
        assert!(split_by_segment(&m, &[]).is_none());
        assert!(split_by_segment(&m, &vec![0..0; CELLS + 1]).is_none());
        assert!(split_by_segment(&m, std::slice::from_ref(&(0..4))).is_none());
    }

    fn quad(uv: bool) -> Mesh {
        let s = CELL_SIZE;
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[1.0, 1.0, 9.0], [s - 1.0, 1.0, 9.0], [s - 1.0, s - 1.0, 9.0], [1.0, s - 1.0, 9.0], [2.0 * s, 1.0, 9.0]]);
        if uv {
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, (0..5).map(|i| [i as f32, 0.5]).collect::<Vec<[f32; 2]>>());
        }
        m.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3, 1, 4, 2]));
        m
    }

    #[test]
    fn terrain_mesh_shading_and_trim() {
        assert!(terrain_mesh(&quad(false), None, 0.0).is_none(), "untextured shape is skipped");
        let m = terrain_mesh(&quad(true), None, 0.0).unwrap();
        let Some(VertexAttributeValues::Float32x3(n)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        assert!(n.iter().all(|v| *v == [0.0, 0.0, 1.0]));
        let Some(VertexAttributeValues::Float32x4(t)) = m.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert!(t.iter().all(|v| *v == [1.0, 0.0, 0.0, 1.0]));
        // Loaded cell (0, 0): the two triangles inside go, the one reaching east stays.
        let id = Affine3A::IDENTITY;
        let m = terrain_mesh(&quad(true), Some((&id, &square((0, 0), (0, 0)))), 0.0).unwrap();
        assert_eq!(m.indices().unwrap().len(), 3);
    }

    #[test]
    fn diffuse_states_from_the_material() {
        let mut images = Assets::<Image>::default();
        let loaded = images.add(Image::default());
        let on_its_way = images.reserve_handle();
        let with = |t: Option<Handle<Image>>| nif_material(StandardMaterial { base_color_texture: t, ..default() });
        assert_eq!(diffuse_of(Some(&with(Some(loaded))), &images), Diffuse::Ready);
        assert_eq!(diffuse_of(Some(&with(Some(on_its_way))), &images), Diffuse::Loading);
        assert_eq!(diffuse_of(Some(&with(None)), &images), Diffuse::Missing, "the white fallback");
        assert_eq!(diffuse_of(None, &images), Diffuse::Loading, "material not in the store yet");
    }

    /// A shape for [`surface`]: its diffuse, and whether the loaded cells cut it all out.
    type Fake = (Diffuse, bool);

    fn surface_of(shapes: &[Fake]) -> Surface {
        surface(shapes, |s: &Fake| s.0, |s: &Fake| s.1)
    }

    #[test]
    fn surface_of_a_chunk() {
        use Diffuse::*;
        assert_eq!(surface_of(&[(Ready, false), (Ready, false)]), Surface::Ready);
        assert_eq!(surface_of(&[(Ready, false), (Loading, false)]), Surface::Pending);
        assert_eq!(surface_of(&[(Loading, false), (Missing, false)]), Surface::Pending, "waits for what is on its way first");
        assert!(matches!(surface_of(&[(Missing, false), (Ready, false)]), Surface::Failed(_)));
        // Nothing textured to draw: empty, not failed.
        assert_eq!(surface_of(&[]), Surface::Empty);
        // Fully trimmed by the loaded cells: empty whatever its diffuse, never failed.
        assert_eq!(surface_of(&[(Missing, true), (Loading, true)]), Surface::Empty);
        // Partly trimmed: the cut shape no longer matters, the drawn one does.
        assert_eq!(surface_of(&[(Missing, true), (Ready, false)]), Surface::Ready);
        assert!(matches!(surface_of(&[(Missing, false), (Ready, true)]), Surface::Failed(_)));
    }

    #[test]
    fn chunk_surface_reads_the_prepared_shapes() {
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<NifMaterial>::default();
        let mat = |materials: &mut Assets<NifMaterial>, t: Option<Handle<Image>>| materials.add(nif_material(StandardMaterial { base_color_texture: t, ..default() }));
        let shape = |m: Mesh, material: Handle<NifMaterial>| PreparedShape {
            mesh: Handle::default(),
            material,
            transform: Affine3A::IDENTITY,
            name: None,
            ancestors: Vec::new(),
            node: None,
            own: Affine3A::IDENTITY,
            cpu: Some(Arc::new(m)),
            segments: Vec::new(),
            external_emittance: false,
            look: nvf_assets::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
            missing_diffuse: None,
            block: 0,
        };
        let white = mat(&mut materials, None);
        let textured = mat(&mut materials, Some(images.add(Image::default())));
        let loading = mat(&mut materials, Some(images.reserve_handle()));
        // The untextured water shape (no UVs) is never drawn, so its lack of a diffuse
        // does not count.
        let model = PreparedModel { shapes: vec![shape(quad(true), textured.clone()), shape(quad(false), white.clone())], ..default() };
        assert_eq!(chunk_surface(&model, &materials, &images, None), Surface::Ready);
        let model = PreparedModel { shapes: vec![shape(quad(true), loading)], ..default() };
        assert_eq!(chunk_surface(&model, &materials, &images, None), Surface::Pending);
        let model = PreparedModel { shapes: vec![shape(quad(true), white.clone())], ..default() };
        assert!(matches!(chunk_surface(&model, &materials, &images, None), Surface::Failed(_)));
        // Cell (0, 0) loaded: the quad's east triangle still draws.
        let cell = square((0, 0), (0, 0));
        assert!(matches!(chunk_surface(&model, &materials, &images, Some(&cell)), Surface::Failed(_)));
        // Cells (0, 0) to (2, 0) loaded: every triangle is cut out, nothing is drawn.
        let cells = square((0, 0), (2, 0));
        assert_eq!(chunk_surface(&model, &materials, &images, Some(&cells)), Surface::Empty);
    }

    /// The camera comes closer: a level-16 chunk gives way to four level-8 quarters. One
    /// whose diffuse is on its way is not spawned, so the level-16 chunk stays drawn (no
    /// untextured quarter, no hole); one whose diffuse failed is spawned (with the fallback
    /// colour) and counts as covering, as a ready one does (#3096).
    #[test]
    fn loading_keeps_the_outgoing_chunk_failed_covers() {
        use Diffuse::*;
        let coarse: Chunk = (16, 0, 0);
        let quarters = [(8, 0, 0), (8, 8, 0), (8, 0, 8), (8, 8, 8)];
        let spawn_with = |of: &dyn Fn(Chunk) -> Surface| -> HashSet<Chunk> { quarters.iter().copied().filter(|&q| admit(&of(q))).collect() };
        let ready = |_: Chunk| surface_of(&[(Ready, false)]);
        let one_loading = |q: Chunk| if q == quarters[1] { surface_of(&[(Loading, false)]) } else { ready(q) };
        let spawned = spawn_with(&one_loading);
        assert!(!spawned.contains(&quarters[1]), "not spawned while its diffuse loads");
        assert!(!covered(coarse, &quarters, |c| spawned.contains(c)), "the outgoing chunk stays drawn");
        let one_failed = |q: Chunk| if q == quarters[1] { surface_of(&[(Missing, false)]) } else { ready(q) };
        let spawned = spawn_with(&one_failed);
        assert!(spawned.contains(&quarters[1]), "a failed diffuse is drawn (fallback colour)");
        assert!(covered(coarse, &quarters, |c| spawned.contains(c)), "and covers like a ready one");
        let one_trimmed = |q: Chunk| if q == quarters[1] { surface_of(&[(Missing, true)]) } else { ready(q) };
        assert_eq!(one_trimmed(quarters[1]), Surface::Empty);
        assert!(covered(coarse, &quarters, |c| spawn_with(&one_trimmed).contains(c)));
    }

    #[test]
    fn fallback_colour_is_not_white() {
        let mut images = Assets::<Image>::default();
        let normal = images.add(Image::default());
        let m = nif_material(StandardMaterial { base_color_texture: None, normal_map_texture: Some(normal.clone()), ..default() });
        let f = fallback_material(&m);
        assert!(f.base.base_color_texture.is_none());
        assert_ne!(f.base.base_color, Color::WHITE, "never Bevy's white");
        let c = f.base.base_color.to_srgba();
        let [r, g, b] = lodgen::NO_BASE_SRGB;
        assert!((c.red - r).abs() < 1e-6 && (c.green - g).abs() < 1e-6 && (c.blue - b).abs() < 1e-6, "the generated LOD's missing-base colour");
        assert_eq!(f.base.normal_map_texture, Some(normal), "the normal map still shades it");
        assert_eq!(f.base.depth_bias, m.base.depth_bias);
    }

    #[test]
    fn texture_failure_names_the_shapes_own_texture() {
        let failures = vec![
            "textures\\landscape\\lod\\wastelandnv\\normals\\wastelandnv.n.level8.x-16.y0.dds: bad".to_string(),
            "textures\\landscape\\lod\\wastelandnv\\diffuse\\wastelandnv.n.level8.x-16.y0.dds: not in any archive".to_string(),
        ];
        let f = texture_failure(&failures, "textures\\landscape\\lod\\wastelandnv\\diffuse\\wastelandnv.n.level8.x-16.y0.dds");
        assert!(f.ends_with("diffuse\\wastelandnv.n.level8.x-16.y0.dds: not in any archive"), "{f}");
        // A path the cache reported nothing for is still named.
        assert_eq!(texture_failure(&failures, "textures\\a.dds"), "textures\\a.dds: not loaded");
    }

    /// Each spawned shape gets its own fallback copy, owned by its handle alone: despawning
    /// the entity drops the last handle and releases the copy; the source material is not
    /// held by it, so it can be evicted as before.
    #[test]
    fn fallback_copy_is_owned_by_the_entity() {
        let mut materials = Assets::<NifMaterial>::default();
        let src = materials.add(nif_material(StandardMaterial::default()));
        let a = fallback_for(&mut materials, &src).unwrap();
        let b = fallback_for(&mut materials, &src).unwrap();
        assert_ne!(a.id(), b.id(), "no shared cache entry");
        for h in [&a, &b, &src] {
            let Handle::Strong(arc) = h else { panic!("strong handle") };
            assert_eq!(Arc::strong_count(arc), 1, "only the entity's handle holds it");
        }
        assert!(materials.get(&a).is_some_and(|m| m.base.base_color_texture.is_none() && m.base.base_color != Color::WHITE));
        assert!(describe_base(&materials.get(&a).unwrap().base).starts_with("base colour sRGB 0.45 0.42 0.38, no base texture"));
        assert!(fallback_for(&mut materials, &Handle::default()).is_none(), "no source material: none");
    }

    #[test]
    fn skirts_hang_from_open_edges() {
        // The three triangles have 5 open edges (two shared: 0-2 and 1-2).
        let m = terrain_mesh(&quad(true), None, 300.0).unwrap();
        assert_eq!(m.count_vertices(), 5 + 2 * 5);
        assert_eq!(m.indices().unwrap().len(), 9 + 6 * 5);
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert!(p[5..].iter().all(|v| v[2] == 9.0 - 300.0));
        // Hanging vertices keep their edge vertex's UV.
        let Some(VertexAttributeValues::Float32x2(uv)) = m.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        assert_eq!(uv.len(), p.len());
        for (k, v) in p.iter().enumerate().skip(5) {
            let src = (0..5).find(|&i| p[i][0] == v[0] && p[i][1] == v[1]).unwrap();
            assert_eq!(uv[k], uv[src]);
        }
        // Every skirt triangle faces outwards: away from the mesh's centre.
        let idx: Vec<u32> = match m.indices().unwrap() {
            Indices::U32(i) => i.clone(),
            Indices::U16(_) => unreachable!(),
        };
        let centre = Vec3::new(CELL_SIZE * 0.8, CELL_SIZE * 0.4, 0.0);
        for t in idx[9..].chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from_array(p[t[i] as usize]));
            let n = (b - a).cross(c - a);
            let mid = (a + b + c) / 3.0;
            assert!(n.z.abs() < 1e-3, "vertical");
            assert!(n.truncate().dot((mid - centre).truncate()) > 0.0, "outwards {t:?}");
        }
        assert_eq!(skirt_depth(4), 512.0);
    }
}
