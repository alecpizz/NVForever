//! Experimental hardware ray-traced lighting through Bevy Solari (`--solari`;
//! docs/design/graphics-solari.md). An engineering choice, not the original game's.
//!
//! Solari lights the deferred G-buffer with ray-traced direct light (the sun, emissive
//! surfaces) and multi-bounce global illumination against a ray-tracing scene of
//! `RaytracingMesh3d` + `StandardMaterial` instances. The game's shapes carry vertex
//! colours and custom materials, which Solari's scene does not take, so each shape gets a
//! hidden child: a copy of its mesh with exactly position, normal, UV and tangent
//! (32-bit indices) and a `StandardMaterial` copied from the shape's base material. The
//! child has no `Mesh3d`, so it is traced but never rasterized.
//!
//! Solari's defaults assume 1 unit = 1 metre; the game's unit is about 1.43 cm, so its
//! distances are scaled by [`UNITS_PER_METRE`].

use crate::fly::FlyCam;
use bevy::camera::CameraMainTextureUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::platform::collections::HashMap;
use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};
use nvf_assets::nif_material::NifMaterial;

/// Stops of exposure added under Solari (see [`setup_camera`]).
const EXPOSURE_LIFT: f32 = 1.3;
/// Game units per metre (FNV's unit is about 1.43 cm).
pub const UNITS_PER_METRE: f32 = 70.0;
/// The most triangles an emissive mesh may have in Solari (it panics past 65,535).
const MAX_EMISSIVE_TRIANGLES: usize = 65_535;

/// Whether Solari is on (set from `--solari` before the plugins are built).
#[derive(Resource, Clone, Copy)]
pub struct SolariOn;

pub struct SolariExperimentPlugin;

impl Plugin for SolariExperimentPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SolariPlugins)
            .insert_resource(SolariOn)
            .init_resource::<ProxyCache>()
            .add_systems(Startup, setup_sky_light)
            .add_systems(Update, (setup_camera, update_sky_light));
        // `NVF_SOLARI_PROXIES=none|terrain|shapes`: a test aid that leaves parts of the
        // traced scene out (default: all).
        let which = std::env::var("NVF_SOLARI_PROXIES").unwrap_or_default();
        if which != "none" && which != "terrain" {
            app.add_systems(Update, (add_proxies, follow_visibility));
        }
        if which != "none" && which != "shapes" {
            app.add_systems(Update, add_terrain_proxies);
        }
        app
            .add_systems(Last, capture_new_meshes);
    }
}

/// The main camera gets Solari's lighting: no multisampling, a main texture Solari can
/// write, distances scaled to game units.
fn setup_camera(mut commands: Commands, cams: Query<(Entity, Option<&SolariLighting>, &Msaa), With<FlyCam>>) {
    for (e, solari, msaa) in &cams {
        if solari.is_none() {
            let s = UNITS_PER_METRE;
            commands.entity(e).insert((
                SolariLighting {
                    // Spatiotemporal reuse of light samples: without DLSS Ray Reconstruction
                    // (Solari's intended denoiser) the raw lighting is far too noisy.
                    restir: true,
                    world_cache_max_gi_ray_distance: 5_000.0,
                    world_cache_position_base_cell_size: 0.15 * s,
                    world_cache_position_lod_scale: 15.0 * s,
                    ..default()
                },
                CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
                // Physically lit surfaces come out darker than the game's display-value
                // sum (lights.rs): brighter by EXPOSURE_LIFT stops. The display-value path
                // (forward shapes) divides the exposure out, so only Solari's light moves.
                bevy::camera::Exposure { ev100: bevy::camera::Exposure::EV100_BLENDER - EXPOSURE_LIFT },
                // Bevy's TAA accumulates Solari's noisy lighting over frames (it already has
                // the motion vectors Solari needs); DLSS Ray Reconstruction would do better.
                bevy::anti_alias::taa::TemporalAntiAliasing::default(),
            ));
            info!("solari: ray-traced lighting on the main camera");
        }
        if *msaa != Msaa::Off {
            commands.entity(e).insert(Msaa::Off);
        }
    }
}

/// Stripped ray-tracing meshes and standard materials made so far, by source asset.
#[derive(Resource, Default)]
struct ProxyCache {
    meshes: HashMap<AssetId<Mesh>, Option<(Handle<Mesh>, usize)>>,
    /// The proxies' own meshes (not to be stripped again).
    own: bevy::platform::collections::HashSet<AssetId<Mesh>>,
    materials: HashMap<UntypedAssetId, Handle<StandardMaterial>>,
}

/// A shape that has its ray-tracing child (the child, if any), or cannot have one.
#[derive(Component)]
struct RtProxied(Option<Entity>);

/// A copy of `mesh` that Solari's scene takes: position, normal, UV and tangent only,
/// triangle list, 32-bit indices; `None` for what cannot be traced (skinned meshes,
/// strips, no positions). With its triangle count.
fn stripped(mesh: &Mesh) -> Option<(Mesh, usize)> {
    // The game's meshes keep no CPU copy once on the GPU: read only what is still there.
    let attr = |a| mesh.try_attribute_option(a).ok().flatten();
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList || attr(Mesh::ATTRIBUTE_JOINT_INDEX).is_some() {
        return None;
    }
    let positions = match attr(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(p) => p.clone(),
        _ => return None,
    };
    let n = positions.len();
    if n == 0 {
        return None;
    }
    let indices: Vec<u32> = match mesh.try_indices_option().ok()? {
        Some(Indices::U32(i)) => i.clone(),
        Some(Indices::U16(i)) => i.iter().map(|&v| v as u32).collect(),
        None => (0..n as u32).collect(),
    };
    if indices.len() < 3 {
        return None;
    }
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD);
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    match attr(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(v)) if v.len() == n => out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, v.clone()),
        _ => out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]),
    }
    match attr(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(v)) if v.len() == n => out.insert_attribute(Mesh::ATTRIBUTE_UV_0, v.clone()),
        _ => out.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]),
    }
    out.insert_indices(Indices::U32(indices));
    let tangents = match attr(Mesh::ATTRIBUTE_TANGENT) {
        Some(VertexAttributeValues::Float32x4(v)) if v.len() == n => Some(v.clone()),
        _ => None,
    };
    match tangents {
        Some(t) => out.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t),
        None => {
            if out.generate_tangents().is_err() {
                out.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, 1.0]; n]);
            }
        }
    }
    out.enable_raytracing = true;
    let triangles = out.indices().map_or(0, |i| i.len() / 3);
    Some((out, triangles))
}

/// The traced material of a shape from its base `StandardMaterial`: colour, textures,
/// roughness and emissive light (an emissive shape becomes a light; held to Solari's
/// triangle limit). Blended shapes are traced as cut-outs.
fn traced_material(base: &StandardMaterial, triangles: usize) -> StandardMaterial {
    let emissive = if triangles <= MAX_EMISSIVE_TRIANGLES { base.emissive } else { LinearRgba::BLACK };
    let alpha_mode = match base.alpha_mode {
        AlphaMode::Opaque => AlphaMode::Opaque,
        AlphaMode::Mask(c) => AlphaMode::Mask(c),
        _ => AlphaMode::Mask(0.5),
    };
    StandardMaterial {
        base_color: base.base_color,
        base_color_texture: base.base_color_texture.clone(),
        emissive,
        emissive_texture: if triangles <= MAX_EMISSIVE_TRIANGLES { base.emissive_texture.clone() } else { None },
        normal_map_texture: base.normal_map_texture.clone(),
        flip_normal_map_y: base.flip_normal_map_y,
        perceptual_roughness: base.perceptual_roughness,
        metallic: base.metallic,
        reflectance: base.reflectance.min(0.5),
        alpha_mode,
        double_sided: base.double_sided,
        cull_mode: base.cull_mode,
        ..default()
    }
}

/// The ray-traced copy of a mesh, made once per mesh asset: the frame it is added (before
/// it goes to the GPU and its CPU copy is dropped, [`capture_new_meshes`]), or here if it
/// still has its data.
fn proxy_mesh(cache: &mut ProxyCache, meshes: &mut Assets<Mesh>, id: AssetId<Mesh>) -> Option<Option<(Handle<Mesh>, usize)>> {
    if let Some(done) = cache.meshes.get(&id) {
        return Some(done.clone());
    }
    let mesh = meshes.get(id)?;
    let made = stripped(mesh).map(|(m, tris)| (meshes.add(m), tris));
    if let Some((h, _)) = &made {
        cache.own.insert(h.id());
    }
    cache.meshes.insert(id, made.clone());
    Some(made)
}

/// Every mesh added this frame gets its traced copy now, while its data is on the CPU
/// (runs last, before the render world extracts it).
fn capture_new_meshes(mut events: MessageReader<AssetEvent<Mesh>>, mut meshes: ResMut<Assets<Mesh>>, mut cache: ResMut<ProxyCache>) {
    let added: Vec<AssetId<Mesh>> = events
        .read()
        .filter_map(|e| match e {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    for id in added {
        if cache.own.contains(&id) {
            continue;
        }
        cache.meshes.remove(&id);
        proxy_mesh(&mut cache, &mut meshes, id);
    }
}

#[allow(clippy::type_complexity)]
fn add_proxies(
    mut commands: Commands,
    shapes: Query<(Entity, &Mesh3d, &MeshMaterial3d<NifMaterial>, &InheritedVisibility), (Without<RtProxied>, Without<crate::lod::DistantLod>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    nif_materials: Res<Assets<NifMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<ProxyCache>,
) {
    for (e, mesh, material, visible) in &shapes {
        // Shapes that are not drawn (disabled references, markers) are not traced either:
        // Solari knows nothing of visibility, and they would cast shadows unseen.
        if !visible.get() {
            continue;
        }
        let Some(nif) = nif_materials.get(&material.0) else { continue };
        // Unlit and additive shapes (glows, effects, the sky's parts) are not surfaces.
        if nif.base.unlit || matches!(nif.base.alpha_mode, AlphaMode::Add | AlphaMode::Multiply | AlphaMode::Premultiplied) {
            commands.entity(e).insert(RtProxied(None));
            continue;
        }
        let Some(made) = proxy_mesh(&mut cache, &mut meshes, mesh.0.id()) else { continue };
        let Some((rt_mesh, triangles)) = made else {
            commands.entity(e).insert(RtProxied(None));
            continue;
        };
        let key = material.0.id().untyped();
        let traced = cache.materials.entry(key).or_insert_with(|| standard.add(traced_material(&nif.base, triangles))).clone();
        let child = commands.spawn((RaytracingMesh3d(rt_mesh), MeshMaterial3d(traced), ChildOf(e))).id();
        commands.entity(e).insert(RtProxied(Some(child)));
    }
}

/// A shape hidden after its child was made loses the child (and gets one again when it is
/// shown: [`add_proxies`] takes it up once it has no `RtProxied`).
fn follow_visibility(mut commands: Commands, shapes: Query<(Entity, &InheritedVisibility, &RtProxied), Changed<InheritedVisibility>>) {
    for (e, visible, proxied) in &shapes {
        if !visible.get() {
            if let Some(child) = proxied.0 {
                commands.entity(child).despawn();
            }
            commands.entity(e).remove::<RtProxied>();
        }
    }
}

#[allow(clippy::type_complexity)]
fn add_terrain_proxies(
    mut commands: Commands,
    tiles: Query<(Entity, &Mesh3d, &MeshMaterial3d<crate::terrain::TerrainMaterial>), Without<RtProxied>>,
    mut meshes: ResMut<Assets<Mesh>>,
    terrain_materials: Res<Assets<crate::terrain::TerrainMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<ProxyCache>,
) {
    for (e, mesh, material) in &tiles {
        let Some(t) = terrain_materials.get(&material.0) else { continue };
        let Some(made) = proxy_mesh(&mut cache, &mut meshes, mesh.0.id()) else { continue };
        let Some((rt_mesh, triangles)) = made else {
            commands.entity(e).insert(RtProxied(None));
            continue;
        };
        let key = material.0.id().untyped();
        let traced = cache.materials.entry(key).or_insert_with(|| standard.add(traced_material(&t.base, triangles))).clone();
        let child = commands.spawn((RaytracingMesh3d(rt_mesh), MeshMaterial3d(traced), ChildOf(e))).id();
        commands.entity(e).insert(RtProxied(Some(child)));
    }
}

/// Strength of the sky's light (the environment map) against the sun's: the sky's colours
/// are about the sun's brightness, the sky's light a fraction of the sun's.
const SKY_STRENGTH: f32 = 1.0;
/// The sky cubemap's side, in texels (a power of two: Bevy filters it on the GPU).
const SKY_SIZE: u32 = 32;

/// The sky's light for Solari: a cubemap made from the weather's sky colours (zenith,
/// horizon, a darker warm ground below), refreshed as they change.
#[derive(Resource)]
struct SkyLight {
    image: Handle<Image>,
    shown: [f32; 9],
}

fn sky_texel(dir: Vec3, upper: Vec3, horizon: Vec3, ground: Vec3) -> Vec3 {
    let y = dir.normalize().y;
    if y >= 0.0 {
        horizon.lerp(upper, y.sqrt())
    } else {
        horizon.lerp(ground, (-y).powf(0.4))
    }
}

/// The direction of texel (u, v) (-1..1) of cube face `f` (+X, -X, +Y, -Y, +Z, -Z).
fn face_dir(f: usize, u: f32, v: f32) -> Vec3 {
    match f {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
}

fn sky_bytes(upper: Vec3, horizon: Vec3, ground: Vec3) -> Vec<u8> {
    let n = SKY_SIZE as usize;
    let mut out = Vec::with_capacity(n * n * 6 * 8);
    for f in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let c = sky_texel(face_dir(f, u, v), upper, horizon, ground);
                for ch in [c.x, c.y, c.z, 1.0] {
                    out.extend_from_slice(&half::f16::from_f32(ch.max(0.0)).to_le_bytes());
                }
            }
        }
    }
    out
}

fn setup_sky_light(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d { width: SKY_SIZE, height: SKY_SIZE, depth_or_array_layers: 6 },
        bevy::render::render_resource::TextureDimension::D2,
        sky_bytes(Vec3::splat(0.5), Vec3::splat(0.5), Vec3::splat(0.2)),
        bevy::render::render_resource::TextureFormat::Rgba16Float,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(bevy::render::render_resource::TextureViewDescriptor {
        dimension: Some(bevy::render::render_resource::TextureViewDimension::Cube),
        ..default()
    });
    commands.insert_resource(SkyLight { image: images.add(image), shown: [-1.0; 9] });
}

/// The camera's sky light, and the cubemap refilled when the sky's colours moved.
fn update_sky_light(
    mut commands: Commands,
    sky: Option<Res<crate::sky::SkyState>>,
    light: Option<ResMut<SkyLight>>,
    mut images: ResMut<Assets<Image>>,
    cams: Query<(Entity, Has<bevy::light::GeneratedEnvironmentMapLight>), With<FlyCam>>,
) {
    let (Some(sky), Some(mut light)) = (sky, light) else { return };
    use fnv_world::sky::ColorType;
    let lin = |c: [f32; 3]| Vec3::from_array(c);
    // Indoors the cell's ambient colour all round; outdoors the weather's sky.
    let (upper, horizon, ground) = if sky.interior.is_some() {
        let a = lin(sky.color(ColorType::Ambient));
        (a, a, a * 0.6)
    } else {
        let upper = lin(sky.color(ColorType::SkyUpper));
        let horizon = lin(sky.color(ColorType::Horizon));
        let ground = lin(sky.color(ColorType::Ambient)) * Vec3::new(0.55, 0.47, 0.38);
        (upper, horizon, ground)
    };
    let shown = [upper.x, upper.y, upper.z, horizon.x, horizon.y, horizon.z, ground.x, ground.y, ground.z];
    if shown.iter().zip(light.shown).any(|(a, b)| (a - b).abs() > 2e-3) {
        light.shown = shown;
        if let Some(mut img) = images.get_mut(&light.image) {
            img.data = Some(sky_bytes(upper, horizon, ground));
        }
    }
    for (e, has) in &cams {
        if !has {
            commands.entity(e).insert(bevy::light::GeneratedEnvironmentMapLight {
                environment_map: light.image.clone(),
                intensity: crate::sky::light_unit() * SKY_STRENGTH,
                ..default()
            });
        }
    }
}
