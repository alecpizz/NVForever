//! Clean-room JIP LN `AttachExtraCamera` / `ProjectExtraCamera` consumer
//! (`fnv_game::nvse_jip14`, `docs/spec-notes/jip-last-14.md`).
//!
//! Spawns a bounded secondary Bevy `Camera3d` rendering the world (excluding first-person
//! arms) into an `Rgba16Float` `Image` render target and binds that image onto a private
//! cloned `NifMaterial` of the requested target geometry node. When no extra cameras are
//! registered in `Game::extra_cameras`, the system exits immediately with zero queries or
//! allocations [design].

use crate::nif_mirror::{rig_names, root_of, MirrorSystems, RenamedNifNode, RigNames, Rigs, Visuals};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{CameraUpdateSystems, Hdr, ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::SimulationLightSystems;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use fnv_game::nif_blocks::ModelView;
use fnv_game::nvse_jip14::{ExtraCameraTarget, EXTRA_CAMERA_MAX};
use fnv_sim::FormId;
use nvf_assets::actor::ShapeName;
use nvf_assets::NifMaterial;
use nvf_play::script::ScriptState;
use nvf_play::PlayerCamera;
use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

/// Base render order for extra cameras (well before the main camera `0` and 3D scope
/// lens `-4`, so projected textures are up to date before the main pass draws them) [design].
const EXTRA_CAMERA_BASE_ORDER: isize = -10;

/// NIF camera/anchor nodes look along local `+Y` with `+Z` up; Bevy `Camera3d` looks
/// along local `-Z` with `+Y` up (`WorldRoot`'s `game_to_bevy` is `-FRAC_PI_2` around X,
/// so `+FRAC_PI_2` around X recovers Bevy camera axes from a NIF node's world rotation)
/// [hypothesis].
fn nif_node_to_camera_rotation(anchor_rotation: Quat) -> Quat {
    anchor_rotation * Quat::from_rotation_x(FRAC_PI_2)
}

/// Safely derive a unit-scale world camera transform from an anchor node's
/// `GlobalTransform`. Returns `None` for non-finite or zero/degenerate-scale matrices so
/// degenerate NIF nodes never panic in `to_scale_rotation_translation` or emit `NaN`
/// camera orientations [design].
fn safe_camera_transform(anchor: &GlobalTransform) -> Option<Transform> {
    const MIN_SCALE: f32 = 1e-5;
    const MIN_DET: f32 = 1e-12;
    let affine = anchor.affine();
    let translation = Vec3::from(affine.translation);
    if !translation.is_finite() {
        return None;
    }
    let m = Mat3::from(affine.matrix3);
    if !m.is_finite() {
        return None;
    }
    let det = m.determinant();
    let sx = m.x_axis.length();
    let sy = m.y_axis.length();
    let sz = m.z_axis.length();
    if !det.is_finite() || det.abs() < MIN_DET || sx < MIN_SCALE || sy < MIN_SCALE || sz < MIN_SCALE {
        return None;
    }
    let sign = if det < 0.0 { -1.0 } else { 1.0 };
    let rot_mat = Mat3::from_cols(m.x_axis / sx, m.y_axis / sy, (m.z_axis / sz) * sign);
    let rot = Quat::from_mat3(&rot_mat);
    if !rot.is_finite() || rot.length_squared() < 1e-6 {
        return None;
    }
    let rotation = nif_node_to_camera_rotation(rot.normalize());
    if !rotation.is_finite() {
        return None;
    }
    Some(Transform { translation, rotation, scale: Vec3::ONE })
}

/// Marker on Bevy secondary camera entities managed by [`ExtraCameraPlugin`].
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ExtraCameraTag;

/// Per-target-entity ownership record ensuring multi-camera contention on one geometry
/// entity never overwrites `original_material` with another camera's projected clone [design].
struct TargetMaterialOwner {
    original_material: Handle<NifMaterial>,
    projected_material: Handle<NifMaterial>,
}

struct LiveExtraCamera {
    camera_entity: Entity,
    image: Handle<Image>,
    pixel_size: u32,
}

#[derive(Resource, Default)]
pub(crate) struct ExtraCameraState {
    session: Option<Arc<()>>,
    active: HashMap<String, LiveExtraCamera>,
    /// True original vs projected material handles keyed by target geometry entity.
    bound_targets: HashMap<Entity, TargetMaterialOwner>,
    /// Number of non-idle sync passes executed (remains `0` when no extra cameras are
    /// registered; read by tests to verify zero-work idle control) [design].
    #[cfg(test)]
    pub(crate) sync_work_count: u64,
}

pub struct ExtraCameraPlugin;

impl Plugin for ExtraCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ExtraCameraState>().add_systems(
            PostUpdate,
            sync.after(MirrorSystems)
                .after(TransformSystems::Propagate)
                .after(crate::underwater::underwater_fog)
                .before(VisibilitySystems::UpdateFrusta)
                // Active secondary cameras need sun cascades and camera projection
                // updates in the same frame (matching `reflection` and `minimap_render`).
                .before(SimulationLightSystems::UpdateDirectionalLightCascades)
                .before(CameraUpdateSystems),
        );
    }
}

type NodesRead<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Name>,
        Option<&'static Children>,
        Option<&'static ShapeName>,
        Has<RenamedNifNode>,
    ),
>;

type DrawnMaterials<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static MeshMaterial3d<NifMaterial>>,
        Option<&'static crate::mesh_terrain_blend::BlendShape>,
        Option<&'static nvf_assets::nif_material::MaterialOverride>,
    ),
>;

type LiveCameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Camera,
        &'static mut RenderTarget,
        &'static mut Projection,
        &'static mut DistanceFog,
        &'static mut Transform,
        &'static mut GlobalTransform,
    ),
    With<ExtraCameraTag>,
>;

type MainCamQuery<'w, 's> = Query<'w, 's, (&'static Projection, &'static DistanceFog), (With<PlayerCamera>, Without<ExtraCameraTag>)>;

fn target_image(images: &mut Assets<Image>, px: u32) -> Handle<Image> {
    let mut image = Image::new_uninit(
        Extent3d { width: px, height: px, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    images.add(image)
}

fn node_label(e: Entity, nodes: &NodesRead, rig: &RigNames) -> String {
    let entry = nodes.get(e).ok();
    if let Some(name) = entry.as_ref().filter(|n| n.3).and_then(|n| n.0) {
        return name.as_str().to_string();
    }
    if rig.is_root(e) {
        if let Some(n) = rig.name_of(e) {
            return n.to_string();
        }
    }
    entry
        .and_then(|n| n.0.map(|n| n.as_str().to_string()))
        .or_else(|| rig.name_of(e).map(str::to_string))
        .unwrap_or_default()
}

fn walk_read(root: Entity, nodes: &NodesRead, rig: &RigNames) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = Vec::new();
    if rig.is_root(root) {
        out.push(root);
    }
    if let Ok((_, Some(ch), _, _)) = nodes.get(root) {
        stack.extend(ch.iter().rev());
    }
    while let Some(e) = stack.pop() {
        out.push(e);
        if let Ok((_, Some(ch), _, _)) = nodes.get(e) {
            stack.extend(ch.iter().rev());
        }
    }
    if out.is_empty() {
        out.push(root);
    }
    out
}

fn find_node_read(root: Entity, target: &str, nodes: &NodesRead, rig: &RigNames) -> Option<Entity> {
    let all = walk_read(root, nodes, rig);
    let exact = all.iter().copied().find(|&e| node_label(e, nodes, rig) == target);
    if exact.is_some() {
        return exact;
    }
    let ci = all.iter().copied().find(|&e| node_label(e, nodes, rig).eq_ignore_ascii_case(target));
    if ci.is_some() {
        return ci;
    }
    let trimmed = target.trim_start_matches('#');
    all.into_iter().find(|&e| {
        nodes.get(e).ok().and_then(|n| n.2).is_some_and(|shape| {
            shape.0.trim_start_matches('#').eq_ignore_ascii_case(trimmed)
        })
    })
}

fn view_order(r: u32, rigs: &Rigs) -> &'static [ModelView] {
    if r != FormId::PLAYER_REF.0 {
        return &[ModelView::Reference];
    }
    let first_drawn = rigs.1.as_deref().is_some_and(|pb| pb.first_person_drawn());
    if first_drawn {
        &[ModelView::FirstPerson, ModelView::ThirdPerson]
    } else {
        &[ModelView::ThirdPerson, ModelView::FirstPerson]
    }
}

fn resolve_node(r: u32, node: Option<&str>, rigs: &Rigs, visuals: &Visuals, nodes: &NodesRead) -> Option<Entity> {
    for &view in view_order(r, rigs) {
        let Some(root) = root_of(r, view, rigs, visuals) else { continue };
        match node {
            None => return Some(root),
            Some(name) => {
                let rig = rig_names(r, view, rigs);
                if let Some(found) = find_node_read(root, name, nodes, &rig) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn restore_target_material(entity: Entity, owner: TargetMaterialOwner, commands: &mut Commands, drawn: &DrawnMaterials) {
    if let Ok((Some(current), _, _)) = drawn.get(entity) {
        if current.0.id() == owner.projected_material.id() {
            commands.entity(entity).try_insert(MeshMaterial3d(owner.original_material));
        }
    }
}

fn teardown_all(
    active: &mut HashMap<String, LiveExtraCamera>,
    bound_targets: &mut HashMap<Entity, TargetMaterialOwner>,
    commands: &mut Commands,
    drawn: &DrawnMaterials,
) {
    for (_, live) in active.drain() {
        commands.entity(live.camera_entity).try_despawn();
    }
    for (entity, owner) in bound_targets.drain() {
        restore_target_material(entity, owner, commands, drawn);
    }
}

fn bind_target_material(
    target_entity: Entity,
    image: &Handle<Image>,
    bound_targets: &mut HashMap<Entity, TargetMaterialOwner>,
    commands: &mut Commands,
    drawn: &DrawnMaterials,
    materials: &mut Assets<NifMaterial>,
) {
    let Ok((handle, blend, drawing)) = drawn.get(target_entity) else { return };
    let Some(handle) = nvf_assets::nif_surface_blend::source_material(handle, blend) else { return };
    let current_plain = nvf_assets::nif_material::plain_material(handle, drawing).clone();

    if let Some(entry) = bound_targets.get_mut(&target_entity) {
        if current_plain.id() == entry.projected_material.id() {
            if let Some(mut mat) = materials.get_mut(&entry.projected_material) {
                if mat.base.base_color_texture.as_ref().map(Handle::id) != Some(image.id()) {
                    mat.base.base_color_texture = Some(image.clone());
                }
            }
            return;
        }
        // An external system replaced the entity's material while projected: adopt the
        // new external handle as the true original and rebuild the projected clone.
        let Some(mut cloned) = materials.get(&current_plain).cloned() else { return };
        cloned.base.base_color_texture = Some(image.clone());
        cloned.base.unlit = true;
        cloned.base.fog_enabled = false;
        let projected_material = materials.add(cloned);
        commands.entity(target_entity).try_insert(MeshMaterial3d(projected_material.clone()));
        entry.original_material = current_plain;
        entry.projected_material = projected_material;
        return;
    }

    let Some(mut cloned) = materials.get(&current_plain).cloned() else { return };
    cloned.base.base_color_texture = Some(image.clone());
    cloned.base.unlit = true;
    cloned.base.fog_enabled = false;
    let projected_material = materials.add(cloned);
    commands.entity(target_entity).try_insert(MeshMaterial3d(projected_material.clone()));
    bound_targets.insert(
        target_entity,
        TargetMaterialOwner {
            original_material: current_plain,
            projected_material,
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn sync(
    script: Option<Res<ScriptState>>,
    mut state: ResMut<ExtraCameraState>,
    commands: Commands,
    rigs: Rigs,
    visuals: Visuals,
    nodes: NodesRead,
    globals: Query<&GlobalTransform, Without<ExtraCameraTag>>,
    drawn: DrawnMaterials,
    main_cam: MainCamQuery,
    live_cams: LiveCameras,
    images: Option<ResMut<Assets<Image>>>,
    materials: Option<ResMut<Assets<NifMaterial>>>,
) {
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else {
        if !state.active.is_empty() || !state.bound_targets.is_empty() {
            let mut commands = commands;
            let state = &mut *state;
            teardown_all(&mut state.active, &mut state.bound_targets, &mut commands, &drawn);
            state.session = None;
        }
        return;
    };
    let (Some(images), Some(materials)) = (images, materials) else { return };
    sync_game(
        &rt.game,
        &mut state,
        commands,
        &rigs,
        &visuals,
        &nodes,
        &globals,
        &drawn,
        &main_cam,
        live_cams,
        images,
        materials,
    );
}

/// Core extra-camera reconciler separated from `ScriptState` so headless ECS tests can
/// exercise the full attach -> project -> update -> detach -> session-reset pipeline.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_game(
    game: &fnv_game::game::Game<'_>,
    state: &mut ExtraCameraState,
    mut commands: Commands,
    rigs: &Rigs,
    visuals: &Visuals,
    nodes: &NodesRead,
    globals: &Query<&GlobalTransform, Without<ExtraCameraTag>>,
    drawn: &DrawnMaterials,
    main_cam: &MainCamQuery,
    mut live_cams: LiveCameras,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<NifMaterial>>,
) {
    // Fast path: zero work when no extra cameras are registered and none are live.
    if game.extra_cameras().is_empty() && state.active.is_empty() && state.bound_targets.is_empty() {
        return;
    }
    #[cfg(test)]
    {
        state.sync_work_count += 1;
    }

    let cur_session = game.extra_camera_session();
    if state.session.as_ref().is_none_or(|prev| !Arc::ptr_eq(prev, &cur_session)) {
        teardown_all(&mut state.active, &mut state.bound_targets, &mut commands, drawn);
        state.session = Some(cur_session);
    }

    let wanted_keys: HashSet<&str> = game
        .extra_cameras()
        .iter()
        .take(EXTRA_CAMERA_MAX)
        .filter(|(_, cam)| {
            cam.projection
                .as_ref()
                .is_some_and(|p| matches!(p.target, ExtraCameraTarget::Geometry { .. }))
        })
        .map(|(k, _)| k.as_str())
        .collect();

    let stale: Vec<String> = state.active.keys().filter(|k| !wanted_keys.contains(k.as_str())).cloned().collect();
    for key in stale {
        if let Some(live) = state.active.remove(&key) {
            commands.entity(live.camera_entity).try_despawn();
        }
    }

    let (main_far, main_fog) = match main_cam.single() {
        Ok((Projection::Perspective(p), fog)) => (p.far, fog.clone()),
        Ok((_, fog)) => (crate::sky::OUTDOOR_FAR, fog.clone()),
        Err(_) => (crate::sky::OUTDOOR_FAR, crate::sky::initial_fog()),
    };

    // Track the deterministic winning camera per target entity (highest projection
    // sequence, breaking ties by camera key) so two cameras targeting the same mesh never
    // fight over or corrupt its original material handle [design].
    let mut target_winners: HashMap<Entity, (u64, String, Handle<Image>)> = HashMap::new();

    for (idx, (key, cam)) in game.extra_cameras().iter().take(EXTRA_CAMERA_MAX).enumerate() {
        let Some(proj) = cam.projection.as_ref() else { continue };
        let ExtraCameraTarget::Geometry { reference: target_ref, node: target_node } = &proj.target else {
            continue;
        };
        let order = EXTRA_CAMERA_BASE_ORDER - idx as isize;
        let anchor_entity = resolve_node(cam.reference.0, cam.parent_node.as_deref(), rigs, visuals, nodes);
        let target_entity = resolve_node(target_ref.0, Some(target_node.as_str()), rigs, visuals, nodes);
        let cam_tf = anchor_entity.and_then(|e| globals.get(e).ok()).and_then(safe_camera_transform);

        let (Some(cam_tf), Some(target_entity)) = (cam_tf, target_entity) else {
            if let Some(live) = state.active.get_mut(key) {
                if let Ok((mut camera, ..)) = live_cams.get_mut(live.camera_entity) {
                    camera.is_active = false;
                }
            }
            continue;
        };

        let cam_global = GlobalTransform::from(cam_tf);
        let fov_rad = proj.fov_deg.to_radians();

        let img = if let Some(live) = state.active.get_mut(key) {
            if live.pixel_size != proj.pixel_size {
                let new_img = target_image(&mut images, proj.pixel_size);
                live.image = new_img.clone();
                live.pixel_size = proj.pixel_size;
                if let Ok((_, mut rt, ..)) = live_cams.get_mut(live.camera_entity) {
                    *rt = RenderTarget::Image(ImageRenderTarget::from(new_img));
                }
            }
            if let Ok((mut camera, _, mut projection, mut fog, mut tf, mut gtf)) = live_cams.get_mut(live.camera_entity) {
                camera.is_active = true;
                camera.order = order;
                if let Projection::Perspective(p) = &mut *projection {
                    p.fov = fov_rad;
                    p.far = main_far;
                    p.aspect_ratio = 1.0;
                }
                *fog = main_fog.clone();
                *tf = cam_tf;
                *gtf = cam_global;
            }
            live.image.clone()
        } else {
            let img = target_image(&mut images, proj.pixel_size);
            let camera_entity = commands
                .spawn((
                    Name::new(format!("ExtraCamera {}", cam.name)),
                    ExtraCameraTag,
                    Camera3d::default(),
                    Camera { order, is_active: true, ..default() },
                    RenderTarget::Image(ImageRenderTarget::from(img.clone())),
                    Hdr,
                    Tonemapping::Linear,
                    Msaa::Off,
                    Projection::Perspective(PerspectiveProjection {
                        fov: fov_rad,
                        near: 5.0,
                        far: main_far,
                        aspect_ratio: 1.0,
                        ..default()
                    }),
                    main_fog.clone(),
                    // Render world and water only; exclude `ARMS_LAYER` and `DECAL_LAYER`
                    // (avoiding a per-PiP-camera `DepthPrepass` requirement for ForwardDecal) [design].
                    // The weather's flat clouds too while the volumetric clouds take their place in
                    // the main view, as the mirror and the scope's lens (the layer is empty otherwise).
                    RenderLayers::from_layers(&[0, crate::reflection::WATER_LAYER, crate::volumetric_clouds::FLAT_CLOUD_LAYER]),
                    cam_tf,
                    cam_global,
                ))
                .id();
            state.active.insert(
                key.clone(),
                LiveExtraCamera {
                    camera_entity,
                    image: img.clone(),
                    pixel_size: proj.pixel_size,
                },
            );
            img
        };

        let replace = target_winners
            .get(&target_entity)
            .is_none_or(|(seq, prev_key, _)| proj.sequence > *seq || (proj.sequence == *seq && key.as_str() > prev_key.as_str()));
        if replace {
            target_winners.insert(target_entity, (proj.sequence, key.clone(), img));
        }
    }

    // Restore original materials for any target entity no longer claimed by an active camera.
    let unbound: Vec<Entity> = state
        .bound_targets
        .keys()
        .copied()
        .filter(|e| !target_winners.contains_key(e))
        .collect();
    for entity in unbound {
        if let Some(owner) = state.bound_targets.remove(&entity) {
            restore_target_material(entity, owner, &mut commands, drawn);
        }
    }

    // Bind each winning camera's image onto its target entity's material.
    for (target_entity, (_, _, img)) in target_winners {
        bind_target_material(target_entity, &img, &mut state.bound_targets, &mut commands, drawn, &mut materials);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use fnv_esm::raw::{write_subrecords, Subrecord};
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_game::game::{Game, GameConfig, RunCtx};
    use fnv_game::GameData;
    use fnv_script::vm::XVal;
    use fnv_script::FunctionTable;
    use fnv_sim::{FormStore, RefTable, SimConfig, World};
    use nvf_play::RefVisual;
    use std::path::PathBuf;

    const STAT_BASE: u32 = 0x1001;
    const CELL_ID: u32 = 0x2001;
    const STAT_REF: u32 = 0x3001;

    fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
        Subrecord { sig: *sig, data, extended: false }
    }

    fn z(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
        let payload = write_subrecords(subs);
        [
            sig.as_slice(),
            &(payload.len() as u32).to_le_bytes(),
            &flags.to_le_bytes(),
            &fid.to_le_bytes(),
            &[0; 4],
            &15u16.to_le_bytes(),
            &[0; 2],
            &payload,
        ]
        .concat()
    }

    fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
        [b"GRUP".as_slice(), &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
    }

    struct TestWorld {
        forms: FormStore,
        refs: RefTable,
        data: GameData,
    }

    impl TestWorld {
        fn build() -> Self {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv");
            let schema = Schema::load_dir(&dir).expect("load fnv schema");
            let hedr = [&1.34f32.to_le_bytes()[..], &0i32.to_le_bytes(), &0x800u32.to_le_bytes()].concat();
            let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]);
            let stat = record(b"STAT", 0, STAT_BASE, &[sub(b"EDID", z("SynOpticStat"))]);
            let cell = record(b"CELL", 0, CELL_ID, &[sub(b"EDID", z("SynOpticCell")), sub(b"DATA", vec![1])]);
            let posrot: Vec<u8> = [0.0f32; 6].iter().flat_map(|f| f.to_le_bytes()).collect();
            let refr = record(
                b"REFR",
                0x400,
                STAT_REF,
                &[sub(b"EDID", z("OpticRef")), sub(b"NAME", STAT_BASE.to_le_bytes().to_vec()), sub(b"DATA", posrot)],
            );
            let children = group(CELL_ID.to_le_bytes(), 6, &group(CELL_ID.to_le_bytes(), 8, &refr));
            let cells = group(
                *b"CELL",
                0,
                &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat())),
            );
            let bytes = [tes4, group(*b"STAT", 0, &stat), cells].concat();
            let plugins = [Plugin::parse(&bytes, "SynOptic.esm".into()).expect("parse")];
            let lo = LoadOrder::new(&plugins).expect("lo");
            let fn_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
            let fn_json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&fn_path).unwrap()).unwrap();
            let table = FunctionTable::load(&fn_path).expect("load functions.json");
            let forms = FormStore::build(&lo, &schema);
            let refs = RefTable::build(&lo, &schema);
            let data = GameData::build(&lo, &schema, table, &fn_json, &refs);
            Self { forms, refs, data }
        }

        fn game(&self) -> Game<'_> {
            let world = World::new(&self.forms, &self.refs, SimConfig::default());
            let mut g = Game::new(&self.data, world, GameConfig::default());
            g.player_entered(Some(FormId(CELL_ID)));
            g
        }
    }

    fn run_script(g: &mut Game<'_>, expr: &str) -> f64 {
        let src = format!("Begin Function {{ }}\nSetFunctionValue ({expr})\nEnd\n");
        match g.run_query(&src, RunCtx::default()).unwrap() {
            XVal::Num(x) => x,
            XVal::Ref(r) => f64::from(r),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    fn base_app() -> (App, Entity, Entity, Handle<NifMaterial>) {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<Assets<NifMaterial>>()
            .init_resource::<ExtraCameraState>();
        let orig_mat = app
            .world_mut()
            .resource_mut::<Assets<NifMaterial>>()
            .add(nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        let root = app
            .world_mut()
            .spawn((
                Name::new("Scene Root"),
                RefVisual(STAT_REF),
                Transform::from_translation(Vec3::new(10.0, 20.0, 30.0)),
                GlobalTransform::from_translation(Vec3::new(10.0, 20.0, 30.0)),
            ))
            .id();
        let anchor = app
            .world_mut()
            .spawn((
                Name::new("ScopeAnchor"),
                Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
                GlobalTransform::from_translation(Vec3::new(11.0, 22.0, 33.0)),
                ChildOf(root),
            ))
            .id();
        let lens = app
            .world_mut()
            .spawn((
                Name::new("ScopeLens"),
                MeshMaterial3d(orig_mat.clone()),
                Transform::IDENTITY,
                GlobalTransform::IDENTITY,
                ChildOf(root),
            ))
            .id();
        (app, anchor, lens, orig_mat)
    }

    /// Two shapes of a reference drawn flat below its cell, the first with a solid core
    /// (solid_cores.rs, a loose entity): switching the light in the air leaves the shapes'
    /// components and children, the reference's root and the node lookups as they were.
    #[test]
    fn a_solid_core_of_one_flat_shape_does_not_change_the_root() {
        use crate::solid_cores::tests as cores;
        let mut app = cores::app(Some(true));
        let cored = cores::add(&mut app, cores::tank());
        let plain = cores::add(&mut app, nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        let cell = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
        let shapes = [(&cored, "Tank"), (&plain, "Lens")].map(|(m, name)| {
            let e = cores::place(&mut app, m);
            app.world_mut().entity_mut(e).insert((RefVisual(STAT_REF), Name::new(name), ChildOf(cell)));
            e
        });
        let mut resolve = |on: bool| {
            app.insert_resource(crate::medium_air_demand::AirDraws(on));
            app.update();
            assert_eq!(cores::cores(&mut app).len(), usize::from(on));
            let w = app.world_mut();
            let kept = shapes.map(|e| (w.entity(e).archetype().id(), w.get::<Children>(e).map(|c| c.to_vec())));
            let found = w
                .run_system_once(|rigs: Rigs, visuals: Visuals, nodes: NodesRead| {
                    [None, Some("Tank"), Some("Lens")].map(|n| resolve_node(STAT_REF, n, &rigs, &visuals, &nodes))
                })
                .unwrap();
            (kept, found)
        };
        let off = resolve(false);
        assert_eq!(off.1[0], Some(shapes[0]));
        for on in [true, false, true] {
            assert!(resolve(on) == off, "light {on}");
        }
    }

    fn run_sync(app: &mut App, game: &Game<'_>) {
        let game_ptr = game as *const Game<'_> as usize;
        app.world_mut()
            .run_system_once(
                move |mut state: ResMut<ExtraCameraState>,
                      commands: Commands,
                      rigs: Rigs,
                      visuals: Visuals,
                      nodes: NodesRead,
                      globals: Query<&GlobalTransform, Without<ExtraCameraTag>>,
                      drawn: DrawnMaterials,
                      main_cam: MainCamQuery,
                      live_cams: LiveCameras,
                      images: ResMut<Assets<Image>>,
                      materials: ResMut<Assets<NifMaterial>>| {
                    // SAFETY: `game` outlives this synchronous `run_system_once` call.
                    let g = unsafe { &*(game_ptr as *const Game<'_>) };
                    sync_game(g, &mut state, commands, &rigs, &visuals, &nodes, &globals, &drawn, &main_cam, live_cams, images, materials);
                },
            )
            .expect("run sync_game");
    }

    #[test]
    fn idle_zero_work_when_no_extra_cameras_registered() {
        let tw = TestWorld::build();
        let game = tw.game();
        let (mut app, _, lens, orig_mat) = base_app();

        run_sync(&mut app, &game);
        run_sync(&mut app, &game);

        assert_eq!(app.world().resource::<ExtraCameraState>().sync_work_count, 0);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());
    }

    #[test]
    fn attach_project_updates_fov_and_resolution_and_restores_material_on_detach() {
        let tw = TestWorld::build();
        let mut game = tw.game();
        let (mut app, _, lens, orig_mat) = base_app();

        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"Optic\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"Optic\" \"ScopeLens\" 12 256"), 1.0);

        run_sync(&mut app, &game);

        // One secondary camera spawned at anchor world position with 12-deg FoV and 256x256 Rgba16Float target.
        let mut cam_q = app.world_mut().query::<(Entity, &Name, &Camera, &RenderTarget, &Projection, &Transform)>();
        let cams: Vec<_> = cam_q.iter(app.world()).filter(|(_, n, ..)| n.as_str().starts_with("ExtraCamera ")).collect();
        assert_eq!(cams.len(), 1);
        let (cam_e, tag, cam, rt, proj, tf) = cams[0];
        assert_eq!(tag.as_str(), "ExtraCamera Optic");
        assert!(cam.is_active);
        assert_eq!(tf.translation, Vec3::new(11.0, 22.0, 33.0));
        let Projection::Perspective(pp) = proj else { panic!("expected perspective") };
        assert!((pp.fov - 12.0f32.to_radians()).abs() < 1e-5);
        let RenderTarget::Image(irt) = rt else { panic!("expected image render target") };
        let img_256 = irt.handle.clone();
        let img_asset = app.world().resource::<Assets<Image>>().get(&img_256).expect("256 image");
        assert_eq!(img_asset.texture_descriptor.size.width, 256);
        assert_eq!(img_asset.texture_descriptor.size.height, 256);
        assert_eq!(img_asset.texture_descriptor.format, TextureFormat::Rgba16Float);

        // Target geometry material cloned and bound to the render target image.
        let lens_mat_handle = app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.clone();
        assert_ne!(lens_mat_handle.id(), orig_mat.id());
        let lens_mat = app.world().resource::<Assets<NifMaterial>>().get(&lens_mat_handle).unwrap();
        assert_eq!(lens_mat.base.base_color_texture.as_ref().map(Handle::id), Some(img_256.id()));
        assert!(lens_mat.base.unlit);

        // Changing FoV alone updates projection in-place without reallocating the Image target.
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"Optic\" \"ScopeLens\" 6 256"), 1.0);
        run_sync(&mut app, &game);
        let Projection::Perspective(pp) = app.world().get::<Projection>(cam_e).unwrap() else { panic!() };
        assert!((pp.fov - 6.0f32.to_radians()).abs() < 1e-5);
        let RenderTarget::Image(irt_after_fov) = app.world().get::<RenderTarget>(cam_e).unwrap() else { panic!() };
        assert_eq!(irt_after_fov.handle.id(), img_256.id());

        // Changing pixelSize reallocates the Image target and updates both camera and material.
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"Optic\" \"ScopeLens\" 6 512"), 1.0);
        run_sync(&mut app, &game);
        let RenderTarget::Image(irt_512) = app.world().get::<RenderTarget>(cam_e).unwrap() else { panic!() };
        let img_512 = irt_512.handle.clone();
        assert_ne!(img_512.id(), img_256.id());
        assert_eq!(app.world().resource::<Assets<Image>>().get(&img_512).unwrap().texture_descriptor.size.width, 512);
        let lens_mat_512 = app.world().resource::<Assets<NifMaterial>>().get(&lens_mat_handle).unwrap();
        assert_eq!(lens_mat_512.base.base_color_texture.as_ref().map(Handle::id), Some(img_512.id()));

        // Detaching despawns the secondary camera and restores the original material handle.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"Optic\" 0"), 1.0);
        run_sync(&mut app, &game);
        assert!(app.world().get_entity(cam_e).is_err());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());
    }

    #[test]
    fn session_change_despawns_cameras_and_restores_target_material() {
        let tw = TestWorld::build();
        let mut game1 = tw.game();
        let (mut app, _, lens, orig_mat) = base_app();

        assert_eq!(run_script(&mut game1, "OpticRef.AttachExtraCamera \"Optic\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game1, "OpticRef.ProjectExtraCamera \"Optic\" \"ScopeLens\" 10 256"), 1.0);
        run_sync(&mut app, &game1);
        assert_ne!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());

        // A fresh Game instance (e.g. save load) clears live cameras and restores the original material.
        let game2 = tw.game();
        run_sync(&mut app, &game2);
        let mut cam_q = app.world_mut().query::<&ExtraCameraTag>();
        assert_eq!(cam_q.iter(app.world()).count(), 0);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());
    }

    #[test]
    fn degenerate_anchor_transform_deactivates_camera_without_panicking() {
        let tw = TestWorld::build();
        let mut game = tw.game();
        let (mut app, anchor, lens, orig_mat) = base_app();

        // Start with a valid camera, then collapse anchor scale to zero.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"Optic\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"Optic\" \"ScopeLens\" 10 256"), 1.0);
        run_sync(&mut app, &game);

        let mut cam_q = app.world_mut().query_filtered::<Entity, With<ExtraCameraTag>>();
        let cam_e = cam_q.single(app.world()).expect("spawned camera");
        assert!(app.world().get::<Camera>(cam_e).unwrap().is_active);
        assert_ne!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());
        // The weather's flat clouds while the volume draws in the main view; the volume itself never.
        let layers = app.world().get::<RenderLayers>(cam_e).unwrap();
        assert!(layers.intersects(&RenderLayers::layer(crate::volumetric_clouds::FLAT_CLOUD_LAYER)));
        assert!(!layers.intersects(&RenderLayers::layer(crate::volumetric_clouds::VOLUME_LAYER)));

        // Zero-scale anchor must deactivate camera and restore original material without panicking.
        *app.world_mut().get_mut::<GlobalTransform>(anchor).unwrap() = GlobalTransform::from(Transform::from_scale(Vec3::ZERO));
        run_sync(&mut app, &game);
        assert!(!app.world().get::<Camera>(cam_e).unwrap().is_active);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());

        // Non-finite translation on a fresh camera must never spawn or panic.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"Optic\" 0"), 1.0);
        run_sync(&mut app, &game);
        *app.world_mut().get_mut::<GlobalTransform>(anchor).unwrap() = GlobalTransform::from(Transform::from_translation(Vec3::NAN));
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"Optic2\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"Optic2\" \"ScopeLens\" 10 256"), 1.0);
        run_sync(&mut app, &game);
        assert_eq!(app.world_mut().query::<&ExtraCameraTag>().iter(app.world()).count(), 0);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());
    }

    #[test]
    fn two_cameras_contending_on_same_target_restore_true_original_in_both_detach_orders() {
        let tw = TestWorld::build();
        let mut game = tw.game();
        let (mut app, _, lens, orig_mat) = base_app();

        // Order 1: CamA (256px) then CamB (512px, later sequence wins) on ScopeLens; detach CamB then CamA.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamA\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"CamA\" \"ScopeLens\" 12 256"), 1.0);
        run_sync(&mut app, &game);
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamB\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"CamB\" \"ScopeLens\" 6 512"), 1.0);
        run_sync(&mut app, &game);

        let bound_mat = app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.clone();
        assert_ne!(bound_mat.id(), orig_mat.id());
        let bound_tex = app.world().resource::<Assets<NifMaterial>>().get(&bound_mat).unwrap().base.base_color_texture.clone().unwrap();
        assert_eq!(app.world().resource::<Assets<Image>>().get(&bound_tex).unwrap().texture_descriptor.size.width, 512);

        // Detach winner (CamB) first: ScopeLens switches to CamA's 256px image while preserving orig_mat.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamB\" 0"), 1.0);
        run_sync(&mut app, &game);
        let bound_tex_a = app.world().resource::<Assets<NifMaterial>>().get(&bound_mat).unwrap().base.base_color_texture.clone().unwrap();
        assert_eq!(app.world().resource::<Assets<Image>>().get(&bound_tex_a).unwrap().texture_descriptor.size.width, 256);

        // Detach remaining camera (CamA): restores the true original material.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamA\" 0"), 1.0);
        run_sync(&mut app, &game);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), orig_mat.id());

        // Order 2: CamA then CamB; detach CamA first, then CamB — still restores true orig_mat.
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamA\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"CamA\" \"ScopeLens\" 12 256"), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamB\" 1 \"ScopeAnchor\""), 1.0);
        assert_eq!(run_script(&mut game, "OpticRef.ProjectExtraCamera \"CamB\" \"ScopeLens\" 6 512"), 1.0);
        run_sync(&mut app, &game);

        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamA\" 0"), 1.0);
        run_sync(&mut app, &game);
        let bound_mat2 = app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.clone();
        assert_ne!(bound_mat2.id(), orig_mat.id());
        let bound_tex_b = app.world().resource::<Assets<NifMaterial>>().get(&bound_mat2).unwrap().base.base_color_texture.clone().unwrap();
        assert_eq!(app.world().resource::<Assets<Image>>().get(&bound_tex_b).unwrap().texture_descriptor.size.width, 512);

        // External material replacement while bound is adopted as the new true original and restored on detach.
        let external_mat = app
            .world_mut()
            .resource_mut::<Assets<NifMaterial>>()
            .add(nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        app.world_mut().entity_mut(lens).insert(MeshMaterial3d(external_mat.clone()));
        run_sync(&mut app, &game);
        assert_ne!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), external_mat.id());

        assert_eq!(run_script(&mut game, "OpticRef.AttachExtraCamera \"CamB\" 0"), 1.0);
        run_sync(&mut app, &game);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(lens).unwrap().0.id(), external_mat.id());
    }
}
