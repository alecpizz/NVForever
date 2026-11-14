//! Distant trees: beyond the loaded cells, TREE references are drawn as their billboards
//! out to [`LodSettings::trees`](crate::lod::LodSettings) (fTreeLoadDistance).
//!
//! The game ships no pre-built tree LOD [observed: no tree LOD meshes or lists in the
//! archives; every `.spt` has a billboard texture, `textures\trees\billboards\<stem>.dds`,
//! and TREE `BNAM` gives its size, world.md §2]. That distant trees are these billboards,
//! upright and turned towards the camera, out to fTreeLoadDistance, is [hypothesis: the
//! Launcher's presets set fTreeLoadDistance 0 / 25,000 / 26,000 / 40,000 from Low to Very
//! High, observed in the game folder's `low.ini` … `VeryHigh.ini`].
//!
//! Each cell whose closest point lies within the distance gets one mesh per TREE base:
//! a card per reference at its foot, BNAM width × height × the reference's scale, lit
//! with an up normal like the near billboards (world.rs). The cards turn in the vertex
//! shader (tree.wesl `TREE_BILLBOARD`), so the meshes never change. A loaded cell draws
//! its own trees; its billboards crossfade with it (fade.rs, the complementary pattern,
//! culled once the cell is fully drawn). Billboards cast and receive no shadows.

use crate::fade;
use crate::lod::{square_distance, LodState};
use crate::tree::{BillboardMaterial, TreeBillboards};
use crate::world::{bevy_to_game, Streamer, WorldData, WorldRoot};
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use fnv_world::placed::{base_object, placement, tree_billboard};
use fnv_world::{cell_of, CELL_SIZE};
use nvf_assets::model::ModelCtx;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type Grid = (i32, i32);

/// Cells whose billboards are built per frame at most (nearest first).
const CELLS_PER_FRAME: usize = 24;

/// One TREE reference for its billboard: base, foot position (game space), scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub base: u32,
    pub pos: [f32; 3],
    pub scale: f32,
}

/// A TREE base's billboard: material and size (width, height).
type Board = Option<(Handle<BillboardMaterial>, [f32; 2])>;

/// The distant trees drawn, per cell.
#[derive(Default)]
pub struct DistantTrees {
    /// The camera's cell the cells were chosen for.
    centre: Option<Grid>,
    /// Cells wanted, nearest first.
    want: Vec<Grid>,
    /// TREE references per cell (enabled at the start), read once.
    cells: HashMap<Grid, Arc<Vec<Placed>>>,
    bases: HashMap<u32, Board>,
    drawn: HashMap<Grid, DrawnCell>,
    /// Trees drawn as distant billboards.
    pub count: usize,
}

struct DrawnCell {
    entities: Vec<Entity>,
    trees: usize,
    /// The fade step given (complement of the cell's).
    step: u8,
}

impl DistantTrees {
    /// The billboards' entities (hidden while the distant land is parked,
    /// `LodState::park`).
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.drawn.values().flat_map(|d| d.entities.iter().copied())
    }

    /// Remove every billboard (LOD off, interiors, another worldspace).
    pub fn clear(&mut self, commands: &mut Commands) {
        for (_, d) in self.drawn.drain() {
            for e in d.entities {
                commands.entity(e).despawn();
            }
        }
        self.centre = None;
        self.want.clear();
        self.count = 0;
    }
}

/// The cells within `dist` of `p` (closest point), nearest first.
pub fn cells_within(p: Vec2, dist: f32) -> Vec<Grid> {
    if dist <= 0.0 {
        return Vec::new();
    }
    let r = (dist / CELL_SIZE).ceil() as i32 + 1;
    let c = cell_of(p.x, p.y);
    let mut out: Vec<(f32, Grid)> = (-r..=r)
        .flat_map(|dx| (-r..=r).map(move |dy| (c.0 + dx, c.1 + dy)))
        .map(|g| (square_distance(p, g, 1), g))
        .filter(|(d, _)| *d < dist)
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(_, g)| g).collect()
}

/// The TREE references of an exterior cell (its own and the persistent ones standing
/// in it) that are enabled at the start.
fn cell_trees(world: &WorldData, g: Grid) -> Vec<Placed> {
    let mut refs: Vec<u32> = world.grid_cell(g).map(|c| c.refs.clone()).unwrap_or_default();
    if let Some(p) = world.persistent_by_grid.get(&g) {
        refs.extend(p);
    }
    let mut out = Vec::new();
    let mut is_tree: HashMap<u32, bool> = HashMap::new();
    for r in refs {
        let Some(p) = placement(world.lo, world.schema, r) else { continue };
        let Some(base) = p.base else { continue };
        let tree = *is_tree.entry(base).or_insert_with(|| base_object(world.lo, world.schema, base).is_some_and(|b| &b.sig == b"TREE" && b.is_speedtree()));
        if tree && world.enabled(&p, 0) {
            out.push(Placed { base, pos: p.position, scale: p.scale });
        }
    }
    out
}

/// One mesh of upright cards (tree.wesl `TREE_BILLBOARD`) for `trees`, positions relative
/// to `origin`: per card four vertices at the foot, UV_1 = corner offset (x right, y
/// up), UV_0 = the billboard picture (v = 1 at the bottom). Returns the mesh and its
/// bounds in the same space.
pub fn cards(trees: &[Placed], origin: Vec3, [w, h]: [f32; 2]) -> (Mesh, Aabb) {
    let n = trees.len();
    let (mut pos, mut uv, mut corner, mut idx) = (Vec::with_capacity(4 * n), Vec::with_capacity(4 * n), Vec::with_capacity(4 * n), Vec::with_capacity(6 * n));
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for t in trees {
        let foot = Vec3::from_array(t.pos) - origin;
        let (hw, ht) = (w / 2.0 * t.scale, h * t.scale);
        lo = lo.min(foot - Vec3::new(hw, hw, 0.0));
        hi = hi.max(foot + Vec3::new(hw, hw, ht));
        let first = pos.len() as u32;
        // Bottom left, bottom right, top right, top left: counter-clockwise on screen.
        for (cx, cy, u, v) in [(-hw, 0.0, 0.0, 1.0), (hw, 0.0, 1.0, 1.0), (hw, ht, 1.0, 0.0), (-hw, ht, 0.0, 0.0)] {
            pos.push(foot.to_array());
            corner.push([cx, cy]);
            uv.push([u, v]);
        }
        idx.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    if lo.x > hi.x {
        (lo, hi) = (Vec3::ZERO, Vec3::ZERO);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    // Lit like the ground under it, as the near billboards (world.rs).
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4 * n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, corner);
    mesh.insert_indices(Indices::U32(idx));
    (mesh, Aabb::from_min_max(lo, hi))
}

fn board(world: &WorldData, base: u32, models: &mut ModelCtx, materials: &mut Assets<BillboardMaterial>) -> Board {
    let (size, tex) = tree_billboard(world.lo, world.schema, base)?;
    let a = &mut *models;
    let image = a.textures.get(&tex, true, &a.data, &mut a.images, a.formats.as_deref())?;
    let mat = materials.add(BillboardMaterial {
        base: StandardMaterial {
            base_color_texture: Some(image),
            alpha_mode: AlphaMode::Mask(0.5),
            cull_mode: None,
            perceptual_roughness: 0.9,
            reflectance: 0.1,
            ..default()
        },
        extension: TreeBillboards::default(),
    });
    Some((mat, size))
}

#[allow(clippy::too_many_arguments)]
pub fn update_distant_trees(
    mut commands: Commands,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    mut lod: ResMut<LodState>,
    root: Query<Entity, With<WorldRoot>>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut models: ModelCtx,
    mut materials: ResMut<Assets<BillboardMaterial>>,
) {
    let dist = lod.settings.trees;
    let lod = &mut *lod;
    let trees = &mut lod.trees;
    if lod.parked.is_some() {
        // Hidden with the distant land while an interior keeps the outdoors (seamless
        // doors): kept as they are.
        return;
    }
    if !lod.enabled || dist <= 0.0 || streamer.interior.is_some() {
        if !trees.drawn.is_empty() {
            trees.clear(&mut commands);
        }
        return;
    }
    let (Ok(root), Ok(cam)) = (root.single(), camera.single()) else { return };
    let g = bevy_to_game(cam.translation());
    let c = cell_of(g.x, g.y);
    if trees.centre != Some(c) {
        trees.centre = Some(c);
        trees.want = cells_within(g.truncate(), dist);
        let keep: HashSet<Grid> = trees.want.iter().copied().collect();
        let gone: Vec<Grid> = trees.drawn.keys().filter(|g| !keep.contains(g)).copied().collect();
        for g in gone {
            if let Some(d) = trees.drawn.remove(&g) {
                trees.count -= d.trees;
                for e in d.entities {
                    commands.entity(e).despawn();
                }
            }
        }
        // Keep the reference lists of cells a little beyond the range.
        let r = (dist / CELL_SIZE).ceil() as i32 + 3;
        trees.cells.retain(|g, _| (g.0 - c.0).abs() <= r && (g.1 - c.1).abs() <= r);
    }
    // Fade steps of the drawn cells follow their cells (culled when fully drawn).
    for (g, d) in trees.drawn.iter_mut() {
        let k = fade::step(streamer.cell_level(*g));
        if k != d.step {
            d.step = k;
            for &e in &d.entities {
                commands.entity(e).insert(fade::complement(k));
            }
        }
    }
    let missing: Vec<Grid> = trees.want.iter().filter(|g| !trees.drawn.contains_key(g)).take(CELLS_PER_FRAME).copied().collect();
    for g in missing {
        let placed = trees.cells.entry(g).or_insert_with(|| Arc::new(cell_trees(&world, g))).clone();
        let mut by_base: HashMap<u32, Vec<Placed>> = HashMap::new();
        for t in placed.iter() {
            by_base.entry(t.base).or_default().push(*t);
        }
        let mut bases: Vec<u32> = by_base.keys().copied().collect();
        bases.sort_unstable();
        let origin = Vec3::new(g.0 as f32 * CELL_SIZE, g.1 as f32 * CELL_SIZE, 0.0);
        let step = fade::step(streamer.cell_level(g));
        let mut d = DrawnCell { entities: Vec::new(), trees: 0, step };
        for b in bases {
            let board = trees.bases.entry(b).or_insert_with(|| board(&world, b, &mut models, &mut materials)).clone();
            let Some((mat, size)) = board else { continue };
            let list = &by_base[&b];
            let (mesh, aabb) = cards(list, origin, size);
            let e = commands
                .spawn((
                    Name::new(format!("Distant trees {},{}", g.0, g.1)),
                    Mesh3d(models.meshes.add(mesh)),
                    MeshMaterial3d(mat),
                    Transform::from_translation(origin),
                    aabb,
                    NoAutoAabb,
                    NotShadowCaster,
                    NotShadowReceiver,
                    fade::complement(step),
                ))
                .id();
            commands.entity(root).add_child(e);
            d.entities.push(e);
            d.trees += list.len();
        }
        trees.count += d.trees;
        trees.drawn.insert(g, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn cells_in_range_nearest_first() {
        let p = Vec2::new(100.0, 100.0);
        let cells = cells_within(p, 10_000.0);
        assert_eq!(cells[0], (0, 0));
        assert!(cells.iter().all(|&g| square_distance(p, g, 1) < 10_000.0));
        // A disc around the camera's cell: (2, 2) is 11,444 units away, (2, 1) 9,025.
        assert!(cells.contains(&(2, 1)) && !cells.contains(&(2, 2)) && cells.contains(&(-3, 0)));
        let d: Vec<f32> = cells.iter().map(|&g| square_distance(p, g, 1)).collect();
        assert!(d.windows(2).all(|w| w[0] <= w[1]));
        assert!(cells_within(p, 0.0).is_empty());
    }

    #[test]
    fn card_layout() {
        let trees = [Placed { base: 1, pos: [4196.0, 50.0, 300.0], scale: 2.0 }, Placed { base: 1, pos: [5000.0, 900.0, 310.0], scale: 1.0 }];
        let (m, aabb) = cards(&trees, Vec3::new(4096.0, 0.0, 0.0), [100.0, 175.0]);
        assert_eq!(m.count_vertices(), 8);
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert!(p[..4].iter().all(|v| *v == [100.0, 50.0, 300.0]));
        let Some(VertexAttributeValues::Float32x2(c)) = m.attribute(Mesh::ATTRIBUTE_UV_1) else { panic!() };
        // Scaled: half width 100, height 350.
        assert_eq!(&c[..4], &[[-100.0, 0.0], [100.0, 0.0], [100.0, 350.0], [-100.0, 350.0]]);
        let Some(VertexAttributeValues::Float32x2(uv)) = m.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        assert_eq!(&uv[..4], &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        assert_eq!(m.indices().unwrap().len(), 12);
        // Bounds hold both trees at full size.
        assert_eq!(Vec3::from(aabb.min()), Vec3::new(0.0, -50.0, 300.0));
        assert_eq!(Vec3::from(aabb.max()), Vec3::new(954.0, 950.0, 650.0));
    }
}
