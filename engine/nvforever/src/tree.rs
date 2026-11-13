//! SpeedTree trees (TREE references): geometry generated from the `.spt` parameters by
//! `fnv_spt` (docs/spec-notes/speedtree.md §5), scaled to the record's billboard size.
//! Branches use [`BarkMaterial`] with the bark texture; leaf cards use [`LeafMaterial`],
//! which turns every card towards the camera. Both move in the wind (tree.wesl).

use bevy::asset::embedded_asset;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::ecs::system::SystemParam;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use nvf_assets::model::ModelCtx;
use std::collections::HashMap;
use std::sync::Arc;

use crate::world::WorldData;

pub type LeafMaterial = ExtendedMaterial<StandardMaterial, LeafCards>;
pub type BarkMaterial = ExtendedMaterial<StandardMaterial, BarkWind>;
/// Distant tree billboards (treelod.rs).
pub type BillboardMaterial = ExtendedMaterial<StandardMaterial, TreeBillboards>;

const SHADER: &str = "embedded://nvforever/tree.wesl";

/// Wind direction in game space (degrees from +x towards +y): where the wind blows to.
/// The records give none [observed: WTHR DATA has a wind speed but no direction], so it
/// is fixed: 120°, north-north-west (330° clockwise from north), where the original's
/// tumbleweeds rolled around Goodsprings [observed: o27_tumble 2026-11-06, 308–357°;
/// docs/spec-notes/wind.md]. Was 30° [chosen] until then.
const WIND_DIRECTION_DEG: f32 = 120.0;

/// The `TreeWind` uniform of tree.wesl.
#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct WindParams {
    /// xyz: wind direction (Bevy world space, horizontal, unit); w: strength 0-1.
    pub wind: Vec4,
    /// x: unused (time comes from the view globals); y: rock angular speed; z: rustle
    /// angular speed (rad/s); w: tree height in the mesh's own units.
    pub motion: Vec4,
    /// Wind before the most recent change, used only on that rendered frame.
    pub previous_wind: Vec4,
    /// x: rendered frame of the change (FrameCount increments in Last).
    pub change_frame: UVec4,
}

/// Leaf cards: camera-facing, rocking and rustling (vertex shaders of the main pass and
/// the prepass, so depth and shadows match).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct LeafCards {
    #[uniform(100)]
    pub wind: WindParams,
}

/// Bark: rocking with the leaves.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct BarkWind {
    #[uniform(100)]
    pub wind: WindParams,
}

/// Distant tree billboards (treelod.rs): upright cards turned about the vertical towards
/// the camera by the vertex shader (tree.wesl `TREE_BILLBOARD`). They stand still: the
/// wind uniform stays zero.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TreeBillboards {
    #[uniform(100)]
    pub wind: WindParams,
}

impl MaterialExtension for TreeBillboards {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.shader_defs.push("TREE_BILLBOARD".into());
        // Billboards crossfade with the cells whose trees they stand in for (treelod.rs).
        nvf_assets::fade_prepass(descriptor, key.mesh_key);
        nvf_assets::soft_edges(descriptor, key.mesh_key);
        Ok(())
    }
}

impl MaterialExtension for LeafCards {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.shader_defs.push("TREE_LEAVES".into());
        // Trees fade in and out with their cells (fade.rs).
        nvf_assets::fade_prepass(descriptor, key.mesh_key);
        nvf_assets::soft_edges(descriptor, key.mesh_key);
        Ok(())
    }
}

impl MaterialExtension for BarkWind {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.shader_defs.push("TREE_BARK".into());
        // Trees fade in and out with their cells (fade.rs).
        nvf_assets::fade_prepass(descriptor, key.mesh_key);
        nvf_assets::depth_equal(descriptor, key.mesh_key);
        Ok(())
    }
}

pub struct TreePlugin;

impl Plugin for TreePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "tree.wesl");
        app.add_plugins((
            MaterialPlugin::<LeafMaterial>::default(),
            MaterialPlugin::<BarkMaterial>::default(),
            MaterialPlugin::<BillboardMaterial>::default(),
        ))
            .init_resource::<TreeCache>()
            .add_systems(Update, wind.after(crate::sky::update_sky))
            .add_systems(PostUpdate, order_leaves.after(bevy::transform::TransformSystems::Propagate));
    }
}

/// The CNAM rock and rustle speeds as angular speeds (rad/s) [tuned by eye:
/// WastelandShrub01 (rock 0.07, rustle 0.35) rocks about every 4 s and rustles about
/// once a second; trees with 1.0 and 1.0 about twice as fast]. How far trees move is
/// set in tree.wesl.
fn angular_speeds(rock: f32, rustle: f32) -> (f32, f32) {
    let tau = std::f32::consts::TAU;
    (tau * (0.2 + 0.6 * rock.max(0.0)), tau * (0.5 + 1.5 * rustle.max(0.0)))
}

/// The weather's wind: xyz its direction (Bevy world space, horizontal, unit), w its
/// strength 0-1 (WTHR DATA wind speed / 255, faded with the weather; none indoors). Also
/// the grass's wind (grass.rs).
pub fn wind_vector(sky: &crate::sky::SkyState) -> Vec4 {
    let speed = |w: &fnv_world::sky::Weather| w.wind_speed as f32 / 255.0;
    let now = speed(&sky.weather);
    let strength = match &sky.prev {
        Some(p) => speed(p) + (now - speed(p)) * sky.fade.clamp(0.0, 1.0),
        None => now,
    };
    let strength = if sky.interior.is_some() { 0.0 } else { strength };
    // A script's direction (SetWindDirection) in place of the fixed one.
    let a = sky.script_wind.unwrap_or(WIND_DIRECTION_DEG).to_radians();
    // Game (x, y, z) is Bevy (x, z, -y).
    // Steps of 1/1000 so a weather fade touches the materials a few hundred times at most.
    Vec3::new(a.cos(), 0.0, -a.sin()).extend((strength * 1000.0).round() / 1000.0)
}

/// Give every tree material the weather's wind ([`wind_vector`]). Materials are only touched when the wind or the set of
/// trees changes: a changed material is prepared again with every entity drawn with it,
/// which every frame cost a third of the frame rate while cells streamed [observed].
fn wind(
    sky: Res<crate::sky::SkyState>,
    cache: Res<TreeCache>,
    frame: Res<bevy::diagnostic::FrameCount>,
    mut leaves: ResMut<Assets<LeafMaterial>>,
    mut bark: ResMut<Assets<BarkMaterial>>,
    mut last: Local<(Vec4, usize)>,
) {
    let dir = wind_vector(&sky);
    let built = cache.0.len();
    if *last == (dir, built) {
        return;
    }
    *last = (dir, built);
    let set = |w: &mut WindParams| {
        w.previous_wind = w.wind;
        w.change_frame.x = frame.0.wrapping_add(1);
        w.wind = dir;
    };
    for v in cache.0.values().flatten() {
        for p in &v.parts {
            match &p.material {
                PartMaterial::Bark(h) => {
                    // Read first: taking the asset mutably marks it changed.
                    if bark.get(h).is_some_and(|m| m.extension.wind.wind != dir) {
                        if let Some(mut m) = bark.get_mut(h) {
                            set(&mut m.extension.wind);
                        }
                    }
                }
                PartMaterial::Leaves(h) => {
                    // Read first: taking the asset mutably marks it changed.
                    if leaves.get(h).is_some_and(|m| m.extension.wind.wind != dir) {
                        if let Some(mut m) = leaves.get_mut(h) {
                            set(&mut m.extension.wind);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub enum PartMaterial {
    Bark(Handle<BarkMaterial>),
    Leaves(Handle<LeafMaterial>),
}

#[derive(Clone)]
pub struct TreePart {
    pub mesh: Handle<Mesh>,
    pub material: PartMaterial,
    /// Bounds in the tree's own space (leaf cards included).
    pub aabb: Aabb,
    /// Leaf cards: the same cards in each draw order ([`LeafOrder`]); `mesh` is the first.
    pub leaf_orders: Option<Arc<[Handle<Mesh>; LEAF_ORDERS]>>,
}

/// Headings around the trunk that leaf cards have a draw order for.
pub const LEAF_ORDERS: usize = 8;

/// Leaf cards are drawn nearest the camera first. They are alpha-tested, so the GPU tests
/// their depth only after their fragment shader has run (it may discard), and what
/// spares it the cards behind is its coarse per-tile test against the depth already
/// written: the nearer cards must be drawn first. A dense shrub filling a quarter of the
/// view ran 160 million prepass fragments a frame in the cards' authored order, 86 million
/// outermost first (docs/design/frame-rate-audit.md §10). Each leaf mesh comes in
/// [`LEAF_ORDERS`] orders, the cards sorted from the side the camera is on for each
/// heading around the trunk, and [`order_leaves`] picks the one for the camera's heading.
#[derive(Component)]
pub struct LeafOrder {
    pub meshes: Arc<[Handle<Mesh>; LEAF_ORDERS]>,
    pub current: usize,
}

/// The heading (of [`LEAF_ORDERS`]) the camera at `eye` lies in from a tree whose own
/// space `local` is (game space, z up): the nearest one.
pub fn leaf_heading(local: &GlobalTransform, eye: Vec3) -> usize {
    let p = local.affine().inverse().transform_point3(eye);
    let a = p.y.atan2(p.x);
    let step = std::f32::consts::TAU / LEAF_ORDERS as f32;
    ((a / step).round() as i64).rem_euclid(LEAF_ORDERS as i64) as usize
}

/// Each leaf mesh drawn in the order for the camera's heading.
pub fn order_leaves(
    camera: Query<&GlobalTransform, With<nvf_play::PlayerCamera>>,
    mut leaves: Query<(&GlobalTransform, &mut LeafOrder, &mut Mesh3d)>,
) {
    let Ok(eye) = camera.single().map(|c| c.translation()) else { return };
    if !leaf_orders_on() {
        return;
    }
    for (local, mut order, mut mesh) in &mut leaves {
        let k = leaf_heading(local, eye);
        if k != order.current {
            order.current = k;
            mesh.0 = order.meshes[k].clone();
        }
    }
}

/// Whether leaf cards change their draw order with the camera (test aid
/// `NVF_PLAY_TEST_LEAF_ORDER=authored` keeps the first: heading 0's).
fn leaf_orders_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PLAY_TEST_LEAF_ORDER").map_or(true, |v| v != "authored"))
}

/// The index list of leaf cards (each `(first vertex, place)`, four vertices from the
/// first) sorted for heading `k`: the cards furthest towards that heading from the
/// cards' centre first.
fn leaf_indices(cards: &[(u32, Vec3)], k: usize) -> Vec<u32> {
    let n = cards.len().max(1) as f32;
    let centre = cards.iter().fold(Vec3::ZERO, |a, c| a + c.1) / n;
    let a = std::f32::consts::TAU * k as f32 / LEAF_ORDERS as f32;
    let dir = Vec3::new(a.cos(), a.sin(), 0.0);
    let mut sorted: Vec<&(u32, Vec3)> = cards.iter().collect();
    sorted.sort_by(|x, y| (y.1 - centre).dot(dir).total_cmp(&(x.1 - centre).dot(dir)));
    sorted.iter().flat_map(|&&(f, _)| [f, f + 1, f + 2, f, f + 2, f + 3]).collect()
}

/// One TREE base made drawable: parts in tree space (size 1) and the scale to game units.
pub struct TreeVisual {
    pub parts: Vec<TreePart>,
    pub scale: f32,
}

/// Generated trees per TREE FormID; `None` remembers a failure (the caller falls back
/// to the billboard).
#[derive(Resource, Default)]
pub struct TreeCache(HashMap<u32, Option<Arc<TreeVisual>>>);

impl TreeCache {
    /// Trees generated (the stress census).
    pub fn len(&self) -> usize {
        self.0.values().flatten().count()
    }
}

#[derive(SystemParam)]
pub struct TreeCtx<'w> {
    cache: ResMut<'w, TreeCache>,
    leaf_materials: ResMut<'w, Assets<LeafMaterial>>,
    bark_materials: ResMut<'w, Assets<BarkMaterial>>,
}

impl TreeCtx<'_> {
    pub fn visual(&mut self, world: &WorldData, tree: u32, models: &mut ModelCtx) -> Option<Arc<TreeVisual>> {
        if let Some(v) = self.cache.0.get(&tree) {
            return v.clone();
        }
        let made = build(world, tree, models, &mut self.leaf_materials, &mut self.bark_materials).map(Arc::new);
        if made.is_none() {
            warn!("TREE {tree:08X}: no generated geometry, drawing its billboard");
        }
        self.cache.0.insert(tree, made.clone());
        made
    }
}

fn build(
    world: &WorldData,
    tree: u32,
    models: &mut ModelCtx,
    leaf_materials: &mut Assets<LeafMaterial>,
    bark_materials: &mut Assets<BarkMaterial>,
) -> Option<TreeVisual> {
    let rec = fnv_world::placed::tree_record(world.lo, world.schema, tree)?;
    let bytes = models.data.read(&rec.model)?;
    // A broken file (an error or a panic) draws the billboard instead.
    let (spt, g) = match nvf_assets::guard::guarded(|| {
        let spt = fnv_spt::Tree::parse(&bytes).map_err(|e| e.to_string())?;
        let g = fnv_spt::generate(&spt);
        Ok((spt, g))
    }) {
        Ok(v) => v,
        Err(e) => {
            warn!("{}: {e}", rec.model);
            return None;
        }
    };
    let scale = rec.size? / g.width().max(g.top()).max(1e-6);
    let mut parts = Vec::new();
    let (rock, rustle) = angular_speeds(rec.rock_speed.unwrap_or(1.0), rec.rustle_speed.unwrap_or(1.0));
    let wind = WindParams { wind: Vec4::ZERO, motion: Vec4::new(0.0, rock, rustle, g.top()), ..default() };
    // Room for the swing (tree.wesl ROCK: a quarter of the height at the top).
    let sway = 0.3 * g.top();

    if !g.indices.is_empty() {
        let bark = bark_texture(&spt.bark_texture);
        let image = bark.and_then(|b| models.textures.get(&b, true, &models.data, &mut models.images, models.formats.as_deref()));
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD);
        let aabb = bounds(g.positions.iter().copied(), sway);
        if let Some(d) = rec.branch_dimming {
            let c: Vec<[f32; 4]> = g.positions.iter().map(|p| dim_colour(g.brightness(d, p[2]))).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, g.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, g.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, g.uvs.clone());
        mesh.insert_indices(bevy::mesh::Indices::U32(g.indices.clone()));
        let mat = bark_materials.add(BarkMaterial {
            base: StandardMaterial {
                // Forward where the default is deferred (Solari): no deferred path of its own.
                opaque_render_method: bevy::material::OpaqueRendererMethod::Forward,
                base_color: if image.is_some() { Color::WHITE } else { Color::srgb(0.3, 0.25, 0.2) },
                base_color_texture: image,
                perceptual_roughness: 0.9,
                reflectance: 0.2,
                ..default()
            },
            extension: BarkWind { wind },
        });
        parts.push(TreePart { mesh: models.meshes.add(mesh), material: PartMaterial::Bark(mat), aabb, leaf_orders: None });
    }

    if !g.leaves.is_empty() {
        let texture = rec.leaf_texture.as_deref()?;
        let image = models.textures.get(texture, true, &models.data, &mut models.images, models.formats.as_deref())?;
        // Each leaf-map cell's visible part (`trimmed_cells`), worked out once per tree.
        let alpha = trim_leaves().then(|| {
            let path = nvf_assets::texture::resolved_path(texture, |p| models.data.vfs.exists(p));
            models.data.read(&path).and_then(|b| nvf_assets::texture::alpha_map(&b))
        }).flatten();
        let mut kept_parts: HashMap<usize, Option<[f32; 4]>> = HashMap::new();
        let (mut whole_area, mut kept_area) = (0.0f32, 0.0f32);
        let n = g.leaves.len();
        let (mut pos, mut nor, mut uv, mut corner) = (Vec::with_capacity(4 * n), Vec::with_capacity(4 * n), Vec::with_capacity(4 * n), Vec::with_capacity(4 * n));
        let mut colour = Vec::with_capacity(4 * n);
        let mut reach = 0.0f32;
        // Each card's first vertex and its place, for the draw orders (`LeafOrder`).
        let mut cards: Vec<(u32, Vec3)> = Vec::with_capacity(n);
        for l in &g.leaves {
            let [w, h] = l.size;
            let [px, py] = l.pivot;
            let (x0, x1, y0, y1) = (-px * w, (1.0 - px) * w, -py * h, (1.0 - py) * h);
            reach = reach.max(x0.abs()).max(x1.abs()).max(y0.abs()).max(y1.abs());
            // Leaf-map cell corners as stored: bottom right, bottom left, top left, top
            // right (speedtree.md §3, tag 10002); the whole texture without one.
            let c = spt.leaf_uvs.get(l.map).copied().unwrap_or([1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]);
            // Corner k's texture coordinate.
            let at = |k: usize| [c[2 * k], c[2 * k + 1]];
            // The card is cut down to its cell's visible part, the texture coordinates with
            // it, so the same texels land on the same places and the clear rest is never
            // rasterised (bottom left is corner 1, bottom right 0, top left 2).
            let [s0, s1, t0, t1] = match &alpha {
                Some(a) => match *kept_parts.entry(l.map).or_insert_with(|| a.kept_part(at(1), at(0), at(2), LEAF_CLEAR, LEAF_PAD)) {
                    Some(k) => k,
                    None => continue,
                },
                None => [0.0, 1.0, 0.0, 1.0],
            };
            whole_area += w * h;
            kept_area += w * h * (s1 - s0) * (t1 - t0);
            let uv_at = |s: f32, t: f32| {
                let (bl, br, tl) = (at(1), at(0), at(2));
                [bl[0] + s * (br[0] - bl[0]) + t * (tl[0] - bl[0]), bl[1] + s * (br[1] - bl[1]) + t * (tl[1] - bl[1])]
            };
            let (x0, x1, y0, y1) = (x0 + s0 * (x1 - x0), x0 + s1 * (x1 - x0), y0 + t0 * (y1 - y0), y0 + t1 * (y1 - y0));
            let first = pos.len() as u32;
            cards.push((first, Vec3::from_array(l.position)));
            let tint = dim_colour(rec.leaf_dimming.map_or(1.0, |d| g.brightness(d, l.position[2])));
            // Bottom left, bottom right, top right, top left: counter-clockwise on screen.
            for (cx, cy, s, t) in [(x0, y0, s0, t0), (x1, y0, s1, t0), (x1, y1, s1, t1), (x0, y1, s0, t1)] {
                pos.push(l.position);
                nor.push(l.normal);
                uv.push(uv_at(s, t));
                corner.push([cx, cy]);
                colour.push(tint);
            }
        }
        debug!("{}: leaf cards cut to {:.0} % of their area", rec.model, 100.0 * kept_area / whole_area.max(1e-6));
        LEAF_AREA.lock().unwrap_or_else(|e| e.into_inner()).add(whole_area, kept_area);
        let aabb = bounds(pos.iter().copied(), reach + sway);
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, corner);
        if rec.leaf_dimming.is_some() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colour);
        }
        let orders: [Handle<Mesh>; LEAF_ORDERS] = std::array::from_fn(|k| {
            let mut m = mesh.clone();
            m.insert_indices(bevy::mesh::Indices::U32(leaf_indices(&cards, k)));
            models.meshes.add(m)
        });
        let mat = leaf_materials.add(LeafMaterial {
            base: StandardMaterial {
                // Forward where the default is deferred (Solari): no deferred path of its own.
                opaque_render_method: bevy::material::OpaqueRendererMethod::Forward,
                base_color_texture: Some(image),
                alpha_mode: AlphaMode::Mask(0.5),
                cull_mode: None,
                double_sided: false,
                perceptual_roughness: 1.0,
                reflectance: 0.1,
                ..default()
            },
            extension: LeafCards { wind },
        });
        parts.push(TreePart { mesh: orders[0].clone(), material: PartMaterial::Leaves(mat), aabb, leaf_orders: Some(Arc::new(orders)) });
    }
    (!parts.is_empty()).then_some(TreeVisual { parts, scale })
}

/// Leaf cards keep the part of their cell above this alpha (of 255), [`LEAF_PAD`] texels
/// wider: well under the cutoff (0.5), so the soft edge a multisampled view draws from the
/// alpha just under the cutoff (nvf_assets coverage.wesl) is kept, and a coarser mip,
/// which spreads the visible texels, loses at most a sliver at the card's edge.
const LEAF_CLEAR: u8 = 8;
const LEAF_PAD: f32 = 2.0;

/// Whether leaf cards are cut to their visible part (test aid
/// `NVF_PLAY_TEST_LEAF_CARDS=whole` keeps them whole).
fn trim_leaves() -> bool {
    std::env::var("NVF_PLAY_TEST_LEAF_CARDS").map_or(true, |v| v != "whole")
}

/// The leaf cards' area before and after cutting, over the trees built so far.
#[derive(Default)]
pub struct LeafArea {
    pub whole: f64,
    pub kept: f64,
    pub trees: u32,
}

impl LeafArea {
    fn add(&mut self, whole: f32, kept: f32) {
        self.whole += f64::from(whole);
        self.kept += f64::from(kept);
        self.trees += 1;
        if self.trees.is_power_of_two() {
            info!("leaf cards: {} tree models' cards cut to {:.0} % of their area", self.trees, 100.0 * self.kept / self.whole.max(1e-6));
        }
    }
}

pub static LEAF_AREA: std::sync::Mutex<LeafArea> = std::sync::Mutex::new(LeafArea { whole: 0.0, kept: 0.0, trees: 0 });

/// Vertex colour for a dimming brightness. The brightness is a display-space factor (the
/// original shades in display space [hypothesis]); this renderer lights in linear space,
/// where the same darkening is the factor to the power 2.2.
fn dim_colour(brightness: f32) -> [f32; 4] {
    let l = brightness.clamp(0.0, 1.0).powf(2.2);
    [l, l, l, 1.0]
}

/// `textures\trees\branches\<file name of the authored bark path, .dds>` (speedtree.md §6).
fn bark_texture(authored: &str) -> Option<String> {
    let name = authored.rsplit(['\\', '/']).next()?.trim();
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    (!stem.is_empty()).then(|| format!("textures\\trees\\branches\\{}.dds", stem.to_ascii_lowercase()))
}

fn bounds(points: impl Iterator<Item = [f32; 3]>, pad: f32) -> Aabb {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in points {
        lo = lo.min(Vec3::from_array(p));
        hi = hi.max(Vec3::from_array(p));
    }
    if lo.x > hi.x {
        (lo, hi) = (Vec3::ZERO, Vec3::ZERO);
    }
    Aabb::from_min_max(lo - pad, hi + pad)
}

/// Spawn a tree's parts under `parent` with the reference transform `rt` (game space,
/// relative to the parent). Returns the number of parts.
pub fn spawn(commands: &mut Commands, parent: Entity, v: &TreeVisual, rt: Mat4, extra: impl Bundle + Clone) -> usize {
    let t = Transform::from_matrix(rt * Mat4::from_scale(Vec3::splat(v.scale)));
    for p in &v.parts {
        let common = (Mesh3d(p.mesh.clone()), t, p.aabb, NoAutoAabb, extra.clone(), crate::shadow_rate::TreePart);
        match &p.material {
            PartMaterial::Bark(m) => {
                commands.entity(parent).with_child((common, MeshMaterial3d(m.clone())));
            }
            // Leaf cards cast shadows (onto the ground and other objects) but receive
            // none: in the shadow pass they face the light, in the main pass the camera,
            // so a tree's own cards blacken its lower half in hard, mismatched blocks
            // [observed]. The crown darkens by the CNAM dimming instead (vertex colours).
            PartMaterial::Leaves(m) => {
                let mut leaves = commands.entity(parent);
                match &p.leaf_orders {
                    Some(orders) => leaves.with_child((
                        common,
                        MeshMaterial3d(m.clone()),
                        bevy::light::NotShadowReceiver,
                        LeafOrder { meshes: orders.clone(), current: 0 },
                    )),
                    None => leaves.with_child((common, MeshMaterial3d(m.clone()), bevy::light::NotShadowReceiver)),
                };
            }
        }
    }
    v.parts.len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn wind_speeds() {
        let (rock, rustle) = super::angular_speeds(0.07, 0.35);
        let period = |w: f32| std::f32::consts::TAU / w;
        assert!((period(rock) - 4.0).abs() < 0.3, "{}", period(rock));
        assert!((period(rustle) - 1.0).abs() < 0.1, "{}", period(rustle));
        assert_eq!(super::angular_speeds(-1.0, -1.0), super::angular_speeds(0.0, 0.0));
    }

    #[test]
    fn leaf_cards_are_drawn_from_the_heading_side_first() {
        use bevy::prelude::*;
        // Three cards along x: at -1, 0 and +1 (first vertices 0, 4, 8).
        let cards = [(0u32, Vec3::new(-1.0, 0.0, 0.0)), (4, Vec3::ZERO), (8, Vec3::new(1.0, 0.0, 0.0))];
        // Heading 0 is +x: the card at +1 first.
        assert_eq!(super::leaf_indices(&cards, 0), [8, 9, 10, 8, 10, 11, 4, 5, 6, 4, 6, 7, 0, 1, 2, 0, 2, 3]);
        // Heading 4 is -x.
        assert_eq!(&super::leaf_indices(&cards, 4)[..6], [0, 1, 2, 0, 2, 3]);
        // A camera at +y of a tree turned a quarter turn about z lies at its -x: heading 4.
        let tree = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)));
        assert_eq!(super::leaf_heading(&tree, Vec3::new(0.0, 5.0, 0.0)), 0);
        assert_eq!(super::leaf_heading(&tree, Vec3::new(0.0, -5.0, 0.0)), 4);
        assert_eq!(super::leaf_heading(&GlobalTransform::IDENTITY, Vec3::new(-3.0, 0.1, 0.0)), 4);
        assert_eq!(super::leaf_heading(&GlobalTransform::IDENTITY, Vec3::new(1.0, 1.0, 0.0)), 1);
    }

    #[test]
    fn bark_paths() {
        assert_eq!(
            super::bark_texture("C:\\Noah\\Fallout\\Trees\\WastelandShrub01\\\\WastelandShrub01Bark.tga").as_deref(),
            Some("textures\\trees\\branches\\wastelandshrub01bark.dds")
        );
        assert_eq!(super::bark_texture(""), None);
    }
}
