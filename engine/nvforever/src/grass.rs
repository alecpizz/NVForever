//! Grass: plants generated on the landscape textures that name grass types (LTEX GNAM →
//! GRAS), not placed by references. Where they stand is `fnv_world::grass::place`; this
//! module draws them: per cell quadrant and grass type, every plant's copy of the type's
//! model merged into one mesh, drawn with [`GrassMaterial`] (grass.wesl), which sways the
//! plants in the wind and lowers them into the ground beyond the grass distance.
//! Facts, sources and confidence tags: docs/spec-notes/grass.md.

use crate::fade::CellFade;
use crate::world::{bevy_to_game, WorldData};
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use fnv_nif::scene::{mesh_data, scene_objects, Decoded, SceneObject};
use fnv_nif::NifFile;
use fnv_world::grass::{load_grass, place, texture_grasses, Grass, GrassSettings, Plant};
use fnv_world::land::{load_land, Land, LandParts};
use fnv_world::CELL_SIZE;
use nvf_assets::model::ModelCtx;
use std::collections::HashMap;
use std::sync::Arc;

pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassSway>;

const SHADER: &str = "embedded://nvforever/grass.wesl";

/// How far a vertex with sway weight 1 moves at no wind and at full wind (game units):
/// the game's `[Grass] fGrassWindMagnitudeMin` and `fGrassWindMagnitudeMax` [documented:
/// `Fallout_default.ini`; hypothesis: what they scale, grass.md §5].
const WIND_MAGNITUDE: (f32, f32) = (5.0, 125.0);

/// The `GrassParams` uniform of grass.wesl.
#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct GrassParams {
    /// xyz: wind direction (Bevy world space, horizontal, unit); w: how far a vertex with
    /// sway weight 1 moves (game units).
    pub wind: Vec4,
    /// x: wave angular speed (rad/s); y: distance at which the grass starts to go; z:
    /// distance over which it goes (game units); w: unused.
    pub motion: Vec4,
    /// x: factor on the ambient light; y: factor on every other light (the sun).
    pub light: Vec4,
    /// Parameters before the most recent change; used only on that rendered frame.
    pub previous_wind: Vec4,
    pub previous_motion: Vec4,
    /// x: rendered frame of the change (FrameCount increments in Last).
    pub change_frame: UVec4,
}

/// The extension part of [`GrassMaterial`]: the vertex shader of the main pass and the
/// prepass (so the depth matches), and the main pass's fragment shader, which softens
/// the strands' cut-out edges where the view is multisampled (grass.wesl, "Edges").
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct GrassSway {
    #[uniform(100)]
    pub params: GrassParams,
}

impl MaterialExtension for GrassSway {
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
        // Grass fades in and out with its cell (fade.rs).
        nvf_assets::fade_prepass(descriptor, key.mesh_key);
        // A multisampled view with a depth-only prepass: the prepass writes the share of
        // the pixel's samples each blade covers, and the main pass tests depth for
        // equality (nvf_assets `grass_edges`).
        if nvf_assets::grass_edges(descriptor, key.mesh_key) {
            return Ok(());
        }
        // Main pass of any other multisampled view: the fragment's alpha is the share of
        // the pixel's samples it covers.
        let prepass = descriptor.vertex.shader_defs.contains(&"PREPASS_PIPELINE".into());
        if !prepass && descriptor.multisample.count > 1 {
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("GRASS_COVERAGE".into());
                descriptor.multisample.alpha_to_coverage_enabled = true;
            }
        }
        // Without multisampling both passes test the alpha at the cutoff: the main pass
        // tests depth for equality with the prepass (left alone with alpha to coverage).
        nvf_assets::depth_equal(descriptor, key.mesh_key);
        Ok(())
    }
}

/// Grass settings from the command line.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GrassConfig {
    /// Grass grows and is drawn (`--no-grass`; the settings page switches it while the
    /// game runs: off hides what has grown and grows none on the cells loaded from then
    /// on, on shows it again and grows what those cells are owed, [`GrassOwed`]).
    pub enabled: bool,
    /// Grass is fully drawn up to this distance from the camera (the game's
    /// `fGrassStartFadeDistance`) and gone `fade_range` further (`fGrassFadeRange`).
    pub fade_start: f32,
    pub fade_range: f32,
    /// Factors on the ambient light and on the sun's light for grass (grass.md §5).
    pub light: (f32, f32),
    pub generate: GrassSettings,
}

/// One shape of a grass model, kept on the CPU: every plant gets a transformed copy.
struct GrassShape {
    positions: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    /// Vertex colours (linear); the alpha is the sway weight.
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    material: Handle<GrassMaterial>,
}

struct GrassModel {
    shapes: Vec<GrassShape>,
}

/// Grass types, the textures that grow them and their models, loaded on first use.
#[derive(Resource, Default)]
pub struct GrassCache {
    types: Vec<Grass>,
    by_form: HashMap<u32, Option<usize>>,
    /// LTEX FormID → indices into `types`.
    texture_types: HashMap<u32, Vec<usize>>,
    /// Per index into `types`; `None` remembers a failure.
    models: HashMap<usize, Option<Arc<GrassModel>>>,
    /// Plants in the loaded cells, and those of them near enough to be drawn.
    pub plants: usize,
    pub plants_in_range: usize,
    /// Plants generated since the start, and the time spent placing them and building
    /// their meshes (part of the cells' loading time).
    pub plants_generated: usize,
    pub build_ms: f64,
}

impl GrassCache {
    /// Grass models made (the stress census).
    pub fn models(&self) -> usize {
        self.models.values().flatten().count()
    }

    pub fn describe(&self) -> String {
        format!("grass {} plants ({} in range, {} types)", self.plants, self.plants_in_range, self.models.values().flatten().count())
    }
}

/// A grass mesh: the plants of one type in one quadrant of a cell.
#[derive(Component)]
pub struct GrassChunk {
    /// Centre (game space, world) and radius of its bounds.
    centre: Vec3,
    radius: f32,
    plants: u32,
}

/// On a cell whose grass was not grown because grass was switched off: what growing it
/// takes, once grass is switched on ([`grow_owed`]).
#[derive(Component, Clone, Copy, Debug)]
pub struct GrassOwed {
    land: u32,
    grid: (i32, i32),
    water: Option<f32>,
}

#[derive(SystemParam)]
pub struct GrassCtx<'w> {
    config: Res<'w, GrassConfig>,
    cache: ResMut<'w, GrassCache>,
    materials: ResMut<'w, Assets<GrassMaterial>>,
}

pub struct GrassPlugin(pub GrassConfig);

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "grass.wesl");
        app.add_plugins(MaterialPlugin::<GrassMaterial>::default())
            .insert_resource(self.0)
            .init_resource::<GrassCache>()
            .add_systems(Update, (wind.after(crate::sky::update_sky), cull.after(crate::world::stream)))
            // Before the fades are applied, so the new meshes get their cell's range.
            .add_systems(Update, grow_owed.after(crate::world::stream).before(crate::fade::fade_cells).before(cull));
    }
}

/// Waves per minute (GRAS wave period [documented: Construction Set wiki; hypothesis:
/// real minutes, grass.md §5]) as an angular speed in rad/s.
fn wave_speed(wave_period: f32) -> f32 {
    std::f32::consts::TAU * wave_period.max(0.0) / 60.0
}

/// How far a vertex with sway weight 1 moves for a wind strength of 0-1.
fn wind_magnitude(strength: f32) -> f32 {
    WIND_MAGNITUDE.0 + (WIND_MAGNITUDE.1 - WIND_MAGNITUDE.0) * strength.clamp(0.0, 1.0)
}

/// Give every grass material the weather's wind, as the trees get it (tree.rs `wind`),
/// and the distances it goes at (the settings page's grass distance,
/// `draw_distance.rs`): materials are only touched when these or the set of materials
/// change.
fn wind(sky: Res<crate::sky::SkyState>, config: Res<GrassConfig>, cache: Res<GrassCache>, frame: Res<bevy::diagnostic::FrameCount>, mut materials: ResMut<Assets<GrassMaterial>>, mut last: Local<(Vec4, Vec2, usize)>) {
    let dir = crate::tree::wind_vector(&sky);
    let wind = dir.truncate().extend(wind_magnitude(dir.w));
    let fade = Vec2::new(config.fade_start, config.fade_range.max(1.0));
    let built = cache.models.len();
    if *last == (wind, fade, built) {
        return;
    }
    *last = (wind, fade, built);
    for s in cache.models.values().flatten().flat_map(|m| &m.shapes) {
        // Read first: taking the asset mutably marks it changed.
        if materials.get(&s.material).is_some_and(|m| m.extension.params.wind != wind || m.extension.params.motion.yz() != fade) {
            if let Some(mut m) = materials.get_mut(&s.material) {
                m.extension.params.previous_wind = m.extension.params.wind;
                m.extension.params.previous_motion = m.extension.params.motion;
                m.extension.params.change_frame.x = frame.0.wrapping_add(1);
                m.extension.params.wind = wind;
                m.extension.params.motion.y = fade.x;
                m.extension.params.motion.z = fade.y;
            }
        }
    }
}

/// Grass was switched on with cells loaded that have none: grow what they are owed, a
/// cell a frame as streaming loads them.
fn grow_owed(
    mut commands: Commands,
    world: Res<WorldData>,
    mut streamer: ResMut<crate::world::Streamer>,
    owed: Query<(Entity, &GrassOwed)>,
    mut grass: GrassCtx,
    mut models: ModelCtx,
) {
    if !grass.config.enabled {
        return;
    }
    let Some((cell, &o)) = owed.iter().next() else { return };
    commands.entity(cell).remove::<GrassOwed>();
    let Some(land) = load_land(world.lo, world.schema, o.land, LandParts::ALL) else { return };
    grass.spawn(&mut commands, cell, &world, o.land, &land, o.grid, o.water, &mut models);
    if streamer.fade_secs > 0.0 {
        // The new meshes need a fade range like the cell's other meshes (fade.rs).
        streamer.fades.entry(o.grid).and_modify(CellFade::again).or_insert(CellFade::new(1.0, false));
    }
}

/// Hide the grass meshes that lie wholly beyond the grass distance (the shader has
/// lowered all their plants into the ground), or all of them while grass is switched
/// off, and count the plants.
fn cull(
    config: Res<GrassConfig>,
    mut cache: ResMut<GrassCache>,
    camera: Query<&GlobalTransform, crate::fly::WorldCamera>,
    mut chunks: Query<(&GrassChunk, &mut Visibility)>,
) {
    let Ok(cam) = camera.single() else { return };
    let eye = bevy_to_game(cam.translation());
    let reach = config.fade_start + config.fade_range;
    let (mut plants, mut near) = (0, 0);
    for (c, mut v) in &mut chunks {
        let show = config.enabled && eye.distance(c.centre) - c.radius <= reach;
        plants += c.plants as usize;
        near += if show { c.plants as usize } else { 0 };
        v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    if (cache.plants, cache.plants_in_range) != (plants, near) {
        (cache.plants, cache.plants_in_range) = (plants, near);
    }
}

impl GrassCtx<'_> {
    /// Index of a grass type in the cache (loading its record on first use).
    fn kind(&mut self, world: &WorldData, grass: u32) -> Option<usize> {
        if let Some(k) = self.cache.by_form.get(&grass) {
            return *k;
        }
        let k = load_grass(world.lo, world.schema, grass).map(|g| {
            self.cache.types.push(g);
            self.cache.types.len() - 1
        });
        if k.is_none() {
            warn!("grass {grass:08X}: no GRAS record");
        }
        self.cache.by_form.insert(grass, k);
        k
    }

    fn model(&mut self, kind: usize, models: &mut ModelCtx) -> Option<Arc<GrassModel>> {
        if let Some(m) = self.cache.models.get(&kind) {
            return m.clone();
        }
        let g = &self.cache.types[kind];
        let params = GrassParams {
            wind: Vec4::ZERO,
            motion: Vec4::new(wave_speed(g.wave_period), self.config.fade_start, self.config.fade_range.max(1.0), 0.0),
            light: Vec4::new(self.config.light.0, self.config.light.1, 0.0, 0.0),
            ..default()
        };
        let name = g.edid.clone().unwrap_or_else(|| format!("{:08X}", g.form_id));
        let made = match g.model.as_deref().map(model_path) {
            None => Err("no model".to_string()),
            // A panic reading a broken file counts as an error: that grass is not drawn.
            Some(path) => nvf_assets::guard::guarded(|| load_model(&path, models, &mut self.materials, params)).map_err(|e| format!("{path}: {e}")),
        };
        let made = match made {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                warn!("grass {name}: {e}");
                None
            }
        };
        self.cache.models.insert(kind, made.clone());
        made
    }

    /// Generate and draw the grass of one cell's landscape (`land`, the LAND record
    /// `land_id`) under `cell` (an entity at the cell's south-west corner, game space).
    /// With grass switched off the cell is owed it ([`GrassOwed`]).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(&mut self, commands: &mut Commands, cell: Entity, world: &WorldData, land_id: u32, land: &Land, grid: (i32, i32), water: Option<f32>, models: &mut ModelCtx) {
        if !self.config.enabled {
            commands.entity(cell).insert(GrassOwed { land: land_id, grid, water });
            return;
        }
        let t0 = std::time::Instant::now();
        self.grow(commands, cell, world, land, grid, water, models);
        self.cache.build_ms += t0.elapsed().as_secs_f64() * 1000.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn grow(&mut self, commands: &mut Commands, cell: Entity, world: &WorldData, land: &Land, grid: (i32, i32), water: Option<f32>, models: &mut ModelCtx) {
        let max_types = self.config.generate.max_types_per_texture;
        for t in land.layers.iter().filter_map(|l| l.texture) {
            if self.cache.texture_types.contains_key(&t) {
                continue;
            }
            // The first types named, in stored order [hypothesis, grass.md §3].
            let kinds: Vec<usize> = texture_grasses(world.lo, world.schema, t).into_iter().filter_map(|g| self.kind(world, g)).take(max_types).collect();
            self.cache.texture_types.insert(t, kinds);
        }
        let plants = place(land, grid, water, &self.cache.types, &self.cache.texture_types, &self.config.generate);
        if plants.is_empty() {
            return;
        }
        self.cache.plants_generated += plants.len();
        // By quadrant and type: smaller meshes to cull and to leave out beyond the distance.
        let mut groups: HashMap<(usize, usize), Vec<&Plant>> = HashMap::new();
        for p in &plants {
            let q = usize::from(p.position[0] >= CELL_SIZE / 2.0) + 2 * usize::from(p.position[1] >= CELL_SIZE / 2.0);
            groups.entry((q, p.kind)).or_default().push(p);
        }
        let origin = Vec3::new(grid.0 as f32 * CELL_SIZE, grid.1 as f32 * CELL_SIZE, 0.0);
        for ((_, kind), group) in groups {
            let Some(model) = self.model(kind, models) else { continue };
            let fit_to_slope = self.cache.types[kind].fit_to_slope;
            for shape in &model.shapes {
                let (mesh, lo, hi) = merged_mesh(shape, &group, fit_to_slope);
                let chunk = GrassChunk { centre: origin + (lo + hi) / 2.0, radius: (hi - lo).length() / 2.0, plants: group.len() as u32 };
                commands.entity(cell).with_child((
                    Mesh3d(models.meshes.add(mesh)),
                    MeshMaterial3d(shape.material.clone()),
                    Transform::IDENTITY,
                    chunk,
                    // The plants are small and many: they take the sun's shadows but
                    // cast none [chosen].
                    NotShadowCaster,
                ));
            }
        }
    }
}

/// Model space → cell space for one plant: scaled, turned about the vertical, leaned onto
/// the slope, moved to its place.
fn plant_transform(p: &Plant, fit_to_slope: bool) -> Affine3A {
    let lean = if fit_to_slope { Quat::from_rotation_arc(Vec3::Z, Vec3::from_array(p.normal).normalize_or(Vec3::Z)) } else { Quat::IDENTITY };
    Affine3A::from_scale_rotation_translation(
        Vec3::new(p.scale[0], p.scale[0], p.scale[1]),
        lean * Quat::from_rotation_z(p.yaw),
        Vec3::from_array(p.position),
    )
}

/// A stored colour value (display space) as a linear one. The game multiplies texture,
/// vertex colour and light in display space [hypothesis, as for the landscape's vertex
/// colours, terrain.rs]; this renderer multiplies in linear space.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The brightness factor of a plant's shade. The factor is a display-space one
/// [hypothesis, as for the trees' dimming, tree.rs]; vertex colours multiply in linear
/// space, where the same darkening is the factor to the power 2.2.
fn shade_colour(shade: f32) -> f32 {
    shade.clamp(0.0, 1.0).powf(2.2)
}

/// One mesh holding a copy of `shape` for every plant (cell space), and its bounds.
///
/// Attributes: every vertex carries the landscape's normal under the plant, so the plant
/// is lit like the ground it stands on, whichever way its faces point. The models' own
/// normals are not used: most point sideways or down, which under the sun would leave
/// the plants nearly unlit, and the game's grass is evenly lit [observed; hypothesis,
/// grass.md §5]. UV_1 is (sway weight, height above the plant's foot), which grass.wesl
/// uses for the wind and the fade.
fn merged_mesh(shape: &GrassShape, plants: &[&Plant], fit_to_slope: bool) -> (Mesh, Vec3, Vec3) {
    let n = shape.positions.len() * plants.len();
    let (mut pos, mut nor, mut uv, mut uv_b, mut col) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    let mut idx = Vec::with_capacity(shape.indices.len() * plants.len());
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in plants {
        let first = pos.len() as u32;
        let t = plant_transform(p, fit_to_slope);
        let shade = shade_colour(p.shade);
        for (i, v) in shape.positions.iter().enumerate() {
            let w = t.transform_point3(*v);
            lo = lo.min(w);
            hi = hi.max(w);
            pos.push(w.to_array());
            nor.push(p.normal);
            uv.push(shape.uvs[i]);
            let c = shape.colors[i];
            uv_b.push([c[3], (w.z - p.position[2]).max(0.0)]);
            col.push([c[0] * shade, c[1] * shade, c[2] * shade, 1.0]);
        }
        idx.extend(shape.indices.iter().map(|i| first + i));
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv_b);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    (mesh, lo, hi)
}

/// Archive path of a GRAS model: `meshes\...`, lower case.
fn model_path(model: &str) -> String {
    let m = model.trim().replace('/', "\\").to_ascii_lowercase();
    let m = m.trim_start_matches('\\');
    if m.starts_with("meshes\\") {
        m.to_string()
    } else {
        format!("meshes\\{m}")
    }
}

fn affine(t: &fnv_nif::scene::Transform) -> Affine3A {
    let m = Mat3::from_cols_array(&t.rotation).transpose(); // stored row-major
    Affine3A::from_mat3_translation(m * t.scale, Vec3::from_array(t.translation))
}

/// The shapes of a grass model with their textures. Grass models are plain triangle
/// shapes whose texture is named by a TallGrassShaderProperty and which are cut out by a
/// NiAlphaProperty alpha test [observed: grass.md §2]; the general model loader
/// (`nvf_assets::model`) reads neither that property's texture nor the vertex alpha.
fn load_model(path: &str, models: &mut ModelCtx, materials: &mut Assets<GrassMaterial>, params: GrassParams) -> Result<GrassModel, String> {
    let bytes = models.data.read(path).ok_or("not in any archive")?;
    let schema = models.data.schema.clone();
    let file = NifFile::parse(&bytes, Some(&schema)).map_err(|e| e.to_string())?;
    let dec = Decoded::new(&file, &bytes, &schema);
    let objects = scene_objects(&file, &dec);
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let mut shapes = Vec::new();
    for o in objects.iter().filter(|o| !o.is_node) {
        // The shape and its ancestors, nearest first.
        let mut chain: Vec<&SceneObject> = vec![o];
        while let Some(&p) = chain.last().and_then(|c| c.parent).and_then(|p| by_block.get(&p)) {
            if chain.len() > 64 {
                break;
            }
            chain.push(p);
        }
        let block = |b: usize| dec.blocks[b].as_ref().ok();
        if chain.iter().any(|c| block(c.block).and_then(|s| s.int("flags")).unwrap_or(0) & 1 != 0) {
            continue; // hidden
        }
        let to_model = chain.iter().fold(Affine3A::IDENTITY, |t, c| affine(&c.transform) * t);
        let Some(data) = block(o.block).and_then(|s| s.block_ref("data")).and_then(|r| dec.get(r)) else { continue };
        let md = mesh_data(data);
        let n = md.positions.len();
        if n == 0 || md.triangles.is_empty() {
            continue;
        }
        // Properties: the nearest object that names one wins.
        let (mut texture, mut cutoff) = (None, None);
        for c in &chain {
            for &r in block(c.block).and_then(|s| s.refs("properties")).unwrap_or_default() {
                let Some(s) = dec.get(r) else { continue };
                match file.blocks[r as usize].type_name.as_str() {
                    "NiAlphaProperty" if cutoff.is_none() => {
                        let (flags, threshold) = (s.int("flags").unwrap_or(0), s.int("threshold").unwrap_or(128));
                        // Bit 9: alpha test; bits 10-12 = 4: "greater than" (nif.xml AlphaFlags).
                        let strict = (flags >> 10) & 7 == 4;
                        cutoff = (flags & 0x200 != 0).then_some((threshold as f32 + if strict { 0.5 } else { 0.0 }) / 255.0);
                    }
                    _ if texture.is_none() && dec.is_a(r as usize, "BSShaderProperty") => {
                        texture = s.text("file_name").map(str::to_string).filter(|t| !t.trim().is_empty());
                    }
                    _ => {}
                }
            }
        }
        // Grass textures are sampled clamped at their edges, not repeated.
        //
        // The grass models share one atlas (`GrassWastelandComp01.dds`, 1024 × 1024, four
        // plants), and several reach a little past it: the blade tips of `GrassWasteland02`,
        // `03` and `04` have V −0.013, −0.1 and −0.008, `GrassWasteland07` reaches U 1.23
        // [observed: `fnvtool nif decode`]. Repeated, those tips sample the far edge of the
        // atlas: its bottom rows are the dense feet of the plants below (mean alpha 66–70 of
        // 255, about 300 of 1,024 texels above alpha 100), while its top row and its left and right
        // columns are empty (alpha 0) [observed: the texture decoded from `Fallout - Textures2.bsa`].
        // So every such tip carried a small opaque triangle of blade feet, floating above the
        // plant (owner's playtest, 2026-10-12, `grasspixels`). The game shows none, which
        // clamping explains [hypothesis: the game's grass sampler clamps; the shader property
        // stores no clamp mode].
        let image = texture.as_deref().and_then(|t| models.textures.get_clamped(t, true, &models.data, &mut models.images, models.formats.as_deref()));
        if image.is_none() {
            warn!("{path}: grass texture {texture:?} not loaded");
        }
        let clean = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
        let colors = if md.colors.len() == n {
            md.colors.iter().map(|c| [srgb_to_linear(clean(c[0])), srgb_to_linear(clean(c[1])), srgb_to_linear(clean(c[2])), clean(c[3])]).collect()
        } else {
            vec![[1.0, 1.0, 1.0, 0.0]; n]
        };

        let uvs = md.uv_sets.first().filter(|u| u.len() == n).cloned().unwrap_or_else(|| vec![[0.0, 0.0]; n]);
        let material = materials.add(GrassMaterial {
            base: StandardMaterial {
                // Forward where the default is deferred (Solari): no deferred path of its own.
                opaque_render_method: bevy::material::OpaqueRendererMethod::Forward,
                base_color: if image.is_some() { Color::WHITE } else { Color::srgb(0.55, 0.5, 0.3) },
                base_color_texture: image,
                alpha_mode: AlphaMode::Mask(cutoff.unwrap_or(0.5)),
                // Both faces drawn and lit alike: the normal is the ground's, not the face's.
                cull_mode: None,
                double_sided: false,
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                ..default()
            },
            extension: GrassSway { params },
        });
        shapes.push(GrassShape {
            positions: md.positions.iter().map(|p| to_model.transform_point3(Vec3::from_array(*p))).collect(),
            uvs,
            colors,
            indices: md.triangles.iter().filter(|t| t.iter().all(|&i| (i as usize) < n)).flatten().copied().collect(),
            material,
        });
    }
    if shapes.is_empty() {
        return Err("no shapes".into());
    }
    Ok(GrassModel { shapes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plant(position: [f32; 3], normal: [f32; 3]) -> Plant {
        Plant { kind: 0, position, normal, yaw: 0.0, scale: [1.0, 2.0], shade: 1.0 }
    }

    #[test]
    fn model_paths() {
        assert_eq!(model_path("Landscape\\Grass\\GrassWasteland05.NIF"), "meshes\\landscape\\grass\\grasswasteland05.nif");
        assert_eq!(model_path("meshes/landscape/grass/a.nif"), "meshes\\landscape\\grass\\a.nif");
    }

    #[test]
    fn wave_and_wind() {
        // 60 waves a minute: one a second.
        assert!((wave_speed(60.0) - std::f32::consts::TAU).abs() < 1e-5);
        assert_eq!(wave_speed(-3.0), 0.0);
        assert_eq!(wind_magnitude(0.0), 5.0);
        assert_eq!(wind_magnitude(1.0), 125.0);
        assert_eq!(wind_magnitude(7.0), 125.0);
    }

    #[test]
    fn plants_stand_on_their_spot_and_lean_with_the_slope() {
        // Level ground: the model's up stays up, scaled by the height scale.
        let t = plant_transform(&plant([10.0, 20.0, 30.0], [0.0, 0.0, 1.0]), true);
        assert!((t.transform_point3(Vec3::new(0.0, 0.0, 5.0)) - Vec3::new(10.0, 20.0, 40.0)).length() < 1e-4);
        // A slope facing east: the model's up follows the ground's normal.
        let n = Vec3::new(1.0, 0.0, 1.0).normalize();
        let t = plant_transform(&plant([0.0; 3], n.to_array()), true);
        assert!((t.transform_vector3(Vec3::Z).normalize() - n).length() < 1e-4);
        let t = plant_transform(&plant([0.0; 3], n.to_array()), false);
        assert!((t.transform_vector3(Vec3::Z).normalize() - Vec3::Z).length() < 1e-4);
    }

    #[test]
    fn merged_mesh_copies_the_shape_per_plant() {
        let shape = GrassShape {
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, 10.0)],
            uvs: vec![[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]],
            colors: vec![[0.5, 0.5, 0.5, 0.0], [0.5, 0.5, 0.5, 0.0], [1.0, 1.0, 1.0, 0.15]],
            indices: vec![0, 1, 2],
            material: Handle::default(),
        };
        let (a, b) = (plant([0.0, 0.0, 100.0], [0.0, 0.0, 1.0]), plant([500.0, 0.0, 50.0], [0.0, 0.0, 1.0]));
        let b = Plant { yaw: std::f32::consts::FRAC_PI_2, ..b };
        let (mesh, lo, hi) = merged_mesh(&shape, &[&a, &b], true);
        assert_eq!(mesh.count_vertices(), 6);
        assert_eq!(mesh.indices().map(|i| i.iter().collect::<Vec<_>>()), Some(vec![0, 1, 2, 3, 4, 5]));
        assert!((lo - Vec3::new(0.0, 0.0, 50.0)).length() < 1e-4 && (hi - Vec3::new(500.0, 1.0, 120.0)).length() < 1e-4, "{lo} {hi}");
        // The tip: sway weight from the vertex alpha, height above the foot (10 × scale 2).
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv_b)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else { panic!("no UV_1") };
        assert_eq!(uv_b[5], [0.15, 20.0]);
        assert_eq!(uv_b[3], [0.0, 0.0]);
        // Every vertex has the ground's normal.
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!("no normals") };
        assert!(normals.iter().all(|n| *n == [0.0, 0.0, 1.0]));
    }
}
