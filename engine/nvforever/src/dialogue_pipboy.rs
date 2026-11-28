//! A dialogue-owned view of the actual resolved Pip-Boy model.
//! No inventory, first-person arm, raise animation or PipBoy gameplay state is changed.
use crate::actors::ActorWorld;
use crate::pipboy_arm::{self, PipBoyPart, Posed};
use crate::player_body::PlayerBody;
use crate::terminal_view::projection::GlassMap;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::camera::{visibility::RenderLayers, CameraUpdateSystems, ImageRenderTarget, RenderTarget};
use bevy::math::{Affine2, Affine3A};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::transform::TransformSystems;
use fnv_actor::resolve::slot;
use fnv_sim::FormId;
use nvf_assets::{model::{ModelCtx, ModelCache}, nif_material::NifMaterial,
    texture::TextureCache, GameData};
use nvf_play::dialogue_device::{DeviceKind, DeviceView, Request};
use std::sync::Arc;

// Dedicated layer separate from `creator_view::STUDIO_LAYER` (17) and `terminal_view` (16).
const LAYER: usize = 22;
const PLACE: Vec3 = Vec3::new(16384.0, 16384.0, 16384.0);
const FOV: f32 = 0.5;
const UV_ATTEMPTS: i32 = 16;
const FRAME_MARGIN: f32 = 1.05;

type ProjectionKey = (Mat4, Affine3A, Option<Rect>, Option<UVec2>);

#[derive(SystemParam)]
struct Sources<'w> {
    world: Option<Res<'w, ActorWorld>>,
    data: Option<Res<'w, GameData>>,
    body: Option<Res<'w, PlayerBody>>,
    sim: Option<Res<'w, nvf_play::sim::SimState>>,
    script: Option<Res<'w, nvf_play::script::ScriptState>>,
    #[cfg(feature = "dialogue-model-capture")]
    capture: Option<Res<'w, crate::dialogue_capture::CaptureWorn>>,
}

impl Sources<'_> {
    /// Fallback Pip-Boy lookup from the player's actual equipped inventory when
    /// `PlayerBody::worn()` has not yet synced (e.g., first-person or dialogue entry).
    fn sim_pipboy(&self) -> Option<Vec<FormId>> {
        let (sim, world) = (self.sim.as_deref()?, self.world.as_deref()?);
        let player = sim.world.peek(FormId::PLAYER_REF)?;
        let equipped: Vec<FormId> = player
            .inventory
            .as_ref()?
            .equipped()
            .into_iter()
            .filter(|id| world.actors.armors.get(id).is_some_and(|a| a.slots & slot::PIPBOY != 0))
            .collect();
        (!equipped.is_empty()).then_some(equipped)
    }

    fn worn(&self) -> Option<std::borrow::Cow<'_, [FormId]>> {
        if let Some(body) = self.body.as_deref() {
            if !body.worn().is_empty() {
                return Some(std::borrow::Cow::Borrowed(body.worn()));
            }
            if let Some(sim_worn) = self.sim_pipboy() {
                return Some(std::borrow::Cow::Owned(sim_worn));
            }
            return Some(std::borrow::Cow::Borrowed(body.worn()));
        }
        #[cfg(feature = "dialogue-model-capture")]
        if let Some(capture) = self.capture.as_deref() { return Some(std::borrow::Cow::Borrowed(&capture.0)); }
        self.sim_pipboy().map(std::borrow::Cow::Owned)
    }

    fn look(&self) -> Option<&fnv_game::chargen::Look> {
        self.body.as_ref().and(self.script.as_deref())
            .and_then(|s| s.rt.as_ref()).and_then(|rt| rt.game.player.look.as_ref())
    }
}

/// The admitted source, not merely the last worn vector. A real empty body is
/// authoritative and must never fall through to capture equipment.
#[derive(Clone, PartialEq)]
struct Provider {
    body: bool,
    worn: Vec<FormId>,
    look: Option<fnv_game::chargen::Look>,
    change: fnv_actor::Change,
}

impl Provider {
    fn matches(&self, sources: &Sources<'_>) -> bool {
        sources.world.as_ref().is_some_and(|world|
            self.body == sources.body.is_some()
                && sources.worn().as_deref() == Some(self.worn.as_slice())
                && self.look.as_ref() == sources.look()
                && self.change == world.change(FormId::PLAYER_REF.0))
    }

    fn read(sources: &Sources<'_>) -> Option<Self> {
        let world = sources.world.as_deref()?;
        Some(Self {body:sources.body.is_some(), worn:sources.worn()?.into_owned(),
            look:sources.look().cloned(),change:world.change(FormId::PLAYER_REF.0)})
    }
}

#[derive(Resource, Default)]
struct Owned {
    request: Option<Request>,
    attempted: bool,
    provider: Option<Provider>,
    model: Option<String>,
    scene: Option<Scene>,
}

struct Scene {
    root: Entity,
    camera: Entity,
    image: Handle<Image>,
    materials: Vec<Handle<NifMaterial>>,
    screen_material: Handle<NifMaterial>,
    glass: Arc<Posed>,
    blockers: Vec<Arc<Posed>>,
    power_button: Option<Arc<Posed>>,
    uv: Rect,
    projection: Option<ProjectionKey>,
    rejected: Option<ProjectionKey>,
}

fn same(a: &Request, b: &Request) -> bool {
    a.stamp == b.stamp && a.image == b.image && a.camera == b.camera
        && a.target == b.target && a.canvas == b.canvas
}

pub(crate) fn install(app: &mut App) {
    use bevy::ecs::schedule::common_conditions::resource_exists;
    app.init_resource::<Owned>().init_resource::<ModelCache>().init_resource::<TextureCache>()
        // Retire a changed provider before the sole dialogue raw reader runs.
        .add_systems(Update, (retire, ApplyDeferred).chain().before(nvf_play::PlaySet::Scripts))
        .add_systems(Update, prepare
            .run_if(resource_exists::<GameData>)
            .run_if(resource_exists::<ActorWorld>)
            .run_if(resource_exists::<Assets<NifMaterial>>)
            .after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync))
        .add_systems(PostUpdate, publish.run_if(resource_exists::<Assets<NifMaterial>>).after(CameraUpdateSystems)
            .after(TransformSystems::Propagate));
}

fn remove_view(commands: &mut Commands, request: &Request) {
    let stamp = request.stamp;
    // Check again when commands apply, so another kind's newly published view survives.
    commands.queue(move |world: &mut World| {
        if world.get_resource::<DeviceView>().is_some_and(|v|
            v.stamp.kind == DeviceKind::PipBoy && v.stamp == stamp) {
            world.remove_resource::<DeviceView>();
        }
    });
}

type RetireAssets<'w> = (Option<Res<'w, Assets<Image>>>, Option<Res<'w, Assets<NifMaterial>>>);

fn retire(
    mut commands: Commands, request: Option<Res<Request>>, sources: Sources,
    mut owned: ResMut<Owned>,
    cameras: Query<(&Camera,&GlobalTransform,&RenderTarget)>, windows: Query<&Window>,
    (images, materials): RetireAssets<'_>,
) {
    let current = request.as_deref().filter(|r| r.valid() && r.stamp.kind == DeviceKind::PipBoy);
    let changed = match (owned.request.as_ref(), current) {
        (Some(old), Some(new)) => !same(old, new), (None, None) => false, _ => true,
    };
    if current.is_none() && owned.request.is_none() && owned.scene.is_none() { return; }
    let inputs_changed = match &owned.provider {
        Some(provider) => !provider.matches(&sources),
        None => sources.world.is_some() && sources.worn().is_some(),
    };
    let refresh = inputs_changed || sources.world.as_ref().is_some_and(|w| w.is_changed());
    let provider = if refresh { Provider::read(&sources) } else { None };
    let model = if refresh {
        provider.as_ref().filter(|p| !p.worn.is_empty()).and_then(|p| {
            let look = p.look.as_ref().map(nvf_play::creator::actor_look);
            let world = sources.world.as_deref()?;
            if let Some(look) = look.as_ref() { resolved_model_path(world,&p.worn,Some(look)) }
            else { model_path(world,&p.worn) }
        })
    } else { None };
    let provider_changed = inputs_changed || (refresh && model != owned.model);
    let missing_assets = owned.scene.as_ref().is_some_and(|scene|
        images.as_deref().is_none_or(|a| a.get(&scene.image).is_none())
        || materials.as_deref().is_none_or(|a| scene.materials.iter().any(|h| a.get(h).is_none())));
    if changed || provider_changed || missing_assets {
        if let Some(old) = owned.request.as_ref() { remove_view(&mut commands, old); }
        if let Some(scene) = owned.scene.take() {
            // Despawn the layer-22 camera and meshes BEFORE removing their image/material
            // assets so no live render target or bindless material slab entry is freed
            // while an entity still references it.
            let root = scene.root;
            let image_id = scene.image.id();
            let material_ids: Vec<_> = scene.materials.iter().map(Handle::id).collect();
            commands.queue(move |world: &mut World| {
                if let Ok(entity) = world.get_entity_mut(root) { entity.despawn(); }
                if let Some(mut images) = world.get_resource_mut::<Assets<Image>>() {
                    images.remove(image_id);
                }
                if let Some(mut materials) = world.get_resource_mut::<Assets<NifMaterial>>() {
                    for id in material_ids { materials.remove(id); }
                }
            });
        }
        owned.attempted = false;
    }
    if changed { owned.request = current.cloned(); }
    if refresh { owned.provider = provider; owned.model = model; }
    // Retry a stable missing asset only when its actual GameData provider changes.
    if sources.data.as_ref().is_some_and(|data| data.is_changed()) { owned.attempted = false; }
    if let (Some(scene), Some(request)) = (owned.scene.as_mut(), current) {
        let signature = camera_signature(scene, request, &cameras, &windows);
        if signature.is_none() || scene.projection.is_some_and(|old| Some(old) != signature) {
            scene.projection = None;
            scene.rejected = None;
            remove_view(&mut commands,request);
        }
    }

}

/// This is the same actor-part resolver the player body uses, without changing it.
pub(crate) fn model_path(world: &ActorWorld, worn: &[FormId]) -> Option<String> {
    resolved_model_path(world, worn, None)
}

fn resolved_model_path(world: &ActorWorld, worn: &[FormId], look: Option<&fnv_actor::Look>) -> Option<String> {
    let appearance = world.resolve_as(FormId::PLAYER_BASE.0, FormId::PLAYER_REF.0,
        Some(worn), look).ok()?;
    appearance.parts.into_iter().find(|p| p.role == "armor"
        && world.actors.armors.get(&p.form).is_some_and(|a| a.slots & slot::PIPBOY != 0))
        .map(|p| p.model)
}

/// Unlit, fully opaque screen material for the offscreen Wrist Terminal dialogue
/// camera. Unlike `pipboy_arm::menus_material` (which sets `pipboy_screen = true`
/// so `nif_material.wesl` writes `alpha = 0.25` for main-camera `imagespace.wesl`
/// `SCREEN_MARK`), the offscreen dialogue image is alpha-composited over the world
/// by UI `ImageNode`, so it must write `alpha = 1.0` to keep the CRT opaque.
pub(crate) fn dialogue_screen_material(source: &NifMaterial, image: Handle<Image>) -> NifMaterial {
    let mut m = pipboy_arm::menus_material(source, image);
    m.base.unlit = true;
    m.base.alpha_mode = AlphaMode::Opaque;
    m.base.emissive = LinearRgba::BLACK;
    m.base.emissive_texture = None;
    m.base.normal_map_texture = None;
    m.extension.pipboy_screen = false;
    m.extension.pipboy_a = Vec4::ZERO;
    m.extension.pipboy_b = Vec4::ZERO;
    m.extension.first_person = false;
    m.extension.refract = false;
    m.extension.shade_light = false;
    m.extension.soft = Vec4::ZERO;
    nvf_assets::authored::clear_for_unlit_copy(&mut m);
    m
}

/// Sanitize non-screen Pip-Boy materials (e.g. additive button glows) for the
/// isolated offscreen dialogue camera so they never sample `view_transmission_texture`
/// or inherit first-person depth/refraction/shade-light state.
pub(crate) fn dialogue_accessory_material(source: &NifMaterial) -> Option<NifMaterial> {
    let needs_copy = (source.base.alpha_mode == AlphaMode::Add && !source.extension.add_linear)
        || source.extension.first_person
        || source.extension.refract
        || source.extension.shade_light
        || source.extension.pipboy_screen
        || source.extension.soft != Vec4::ZERO;
    if !needs_copy {
        return None;
    }
    let mut m = source.clone();
    if m.base.alpha_mode == AlphaMode::Add {
        m.extension.add_linear = true;
    }
    m.extension.first_person = false;
    m.extension.refract = false;
    m.extension.shade_light = false;
    m.extension.pipboy_screen = false;
    m.extension.soft = Vec4::ZERO;
    Some(m)
}

/// [design: docs/design/android.md] Frame the entire Wrist Terminal bezel (top rim,
/// side dials/knobs, bottom STATS/ITEMS/DATA housing, and screen opening) straight-on
/// with exact per-vertex perspective bounds and safe near-plane clearance so no part
/// of the Pip-Boy clips against the viewport or near plane.
fn fit(glass: &Posed, shapes: &[Arc<Posed>], aspect: f32) -> Option<(Transform, Vec3, f32, f32)> {
    if !aspect.is_finite() || aspect <= 0.0 { return None; }
    let center = glass.middle()?;
    // Area-weighted average normal across the whole glass mesh avoids corner-triangle tilt.
    let normal = glass.triangles.iter().fold(Vec3::ZERO, |acc, &[a, b, c]| {
        acc + (glass.points[b] - glass.points[a]).cross(glass.points[c] - glass.points[a])
    }).try_normalize()?;
    let uv_center = glass.uvs.iter().copied().sum::<Vec2>() / glass.uvs.len() as f32;
    let here = glass.at_uv(uv_center)?;
    let raw_down = glass.at_uv(uv_center + Vec2::new(0.0, 0.05))
        .and_then(|b| glass.at_uv(uv_center - Vec2::new(0.0, 0.05)).map(|t| b - t))
        .or_else(|| glass.at_uv(uv_center + Vec2::new(0.0, 0.01)).map(|b| b - here))?;
    // Orthogonalize `up` against `normal` so the screen horizon stays completely level.
    let raw_up = -raw_down;
    let up = (raw_up - normal * raw_up.dot(normal)).try_normalize()?;
    let orientation = Transform::from_translation(normal).looking_at(Vec3::ZERO, up);
    if !orientation.rotation.is_finite() { return None; }
    let right = orientation.rotation * Vec3::X;
    let up = orientation.rotation * Vec3::Y;
    let mut glass_height: f32 = 0.0;
    for point in &glass.points {
        glass_height = glass_height.max((*point - center).dot(up).abs());
    }
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for shape in shapes {
        for point in &shape.points {
            let p = *point - center;
            let x = p.dot(right);
            let y = p.dot(up);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    if !min_x.is_finite() || !max_x.is_finite() || !min_y.is_finite() || !max_y.is_finite() {
        return None;
    }
    // Center camera aim on the full housing/bezel midpoint (including the bottom
    // STATS/ITEMS/DATA section and left/right rims) instead of only the glass center.
    let aim = center + right * ((min_x + max_x) * 0.5) + up * ((min_y + max_y) * 0.5);
    let tan_half_y = (FOV * 0.5).tan();
    let tan_half_x = tan_half_y * aspect;
    if !tan_half_x.is_finite() || tan_half_x <= 0.0 || !tan_half_y.is_finite() || tan_half_y <= 0.0 {
        return None;
    }
    let mut required_distance: f32 = 0.0;
    let mut front_depth: f32 = 0.0;
    let mut back_depth: f32 = 0.0;
    for shape in shapes {
        for point in &shape.points {
            let rel = *point - aim;
            let x = rel.dot(right).abs();
            let y = rel.dot(up).abs();
            let z = rel.dot(normal);
            if z > 0.0 {
                front_depth = front_depth.max(z);
            } else {
                back_depth = back_depth.max(-z);
            }
            let need_x = z + (x * FRAME_MARGIN) / tan_half_x;
            let need_y = z + (y * FRAME_MARGIN) / tan_half_y;
            required_distance = required_distance.max(need_x).max(need_y);
        }
    }
    let min_clearance = (glass_height.max(1.0) * 0.25).max(0.5);
    let distance = required_distance.max(front_depth + min_clearance);
    if !distance.is_finite() || distance <= front_depth { return None; }
    let transform = Transform::from_translation(aim + normal * distance).looking_at(aim, up);
    if !transform.translation.is_finite() || !transform.rotation.is_finite() { return None; }
    let clearance = distance - front_depth;
    let near = (clearance * 0.25).clamp(0.05, (clearance * 0.5).max(0.05));
    let far = distance + back_depth + ((max_x - min_x) + (max_y - min_y)).max(1.0);
    Some((transform, aim, near, far))
}

fn prepare(
    mut commands: Commands, sources: Sources,
    mut models: ModelCtx, mut owned: ResMut<Owned>,
    windows: Query<&Window>,
) {
    if owned.scene.is_some() { return; }
    let Some(request) = owned.request.clone() else { return; };
    // The early provider observer is the admission authority; publish checks again.
    if !owned.provider.as_ref().is_some_and(|p| p.matches(&sources)) { return; }
    if owned.attempted { return; }
    let Ok(window) = windows.get(request.stamp.window) else { return; };
    let physical = Rect::from_corners(Vec2::ZERO, window.resolution.physical_size().as_vec2()).inflate(0.5);
    if !window.focused || !physical.contains(request.stamp.display.min)
        || !physical.contains(request.stamp.display.max)
        || request.target != nvf_play::pipboy::TARGET { return; }
    owned.attempted = true;
    let Some(path) = owned.model.clone() else {
        // A character who has not been given a Pip-Boy wears none; the Wrist
        // Terminal theme then keeps the conversation on Normal.
        info!("dialogue Pip-Boy: the player wears no Pip-Boy model; Wrist Terminal dialogue uses Normal");
        return;
    };
    let Some(model) = models.model_with_cpu(&path) else {
        warn!("dialogue Pip-Boy: actual model {path} not loaded"); return;
    };
    // Static prepared models already contain skinned shapes in their actual NIF bind pose.
    let place = Affine3A::from_translation(PLACE)
        * Affine3A::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let mut prepared = Vec::new();
    for shape in &model.shapes {
        if shape.name.as_deref().is_some_and(fnv_actor::mesh::is_light_effect_shape) {
            continue;
        }
        let part = shape.name.as_deref().and_then(pipboy_arm::part);
        // Skip ScreenLit and Glare in dialogue so the text screen stays crisp and opaque
        // without the bright middle glare washout.
        if matches!(part, Some(PipBoyPart::ScreenLit | PipBoyPart::Glare)) { continue; }
        let transform = place * model.root * shape.transform;
        let pose = shape.cpu.as_deref()
            .and_then(|mesh| pipboy_arm::pointer_pose(mesh, None, Mat4::from(transform)))
            .map(Arc::new);
        if part == Some(PipBoyPart::Screen) && pose.is_none() {
            return;
        }
        prepared.push((shape, part, transform, pose));
    }
    let screens = prepared.iter().filter(|(_,p,_,_)| *p == Some(PipBoyPart::Screen)).collect::<Vec<_>>();
    if screens.len() != 1 { return; }
    let Some(glass) = screens[0].3.clone() else { return; };
    let bounds = glass.uvs.iter().copied().fold(None, |bounds: Option<Rect>, uv|
        Some(bounds.map_or(Rect::from_corners(uv,uv), |r| Rect::from_corners(r.min.min(uv),r.max.max(uv)))));
    let Some(uv) = bounds.filter(|r| r.min.is_finite() && r.max.is_finite() && r.width()>0.0 && r.height()>0.0) else { return; };
    let all = prepared.iter().filter_map(|(_,_,_,p)| p.clone()).collect::<Vec<_>>();
    let display = request.stamp.display.size();
    let Some((camera_transform, aim, near, far)) = fit(&glass, &all, display.x/display.y) else { return; };
    let size = display.ceil().as_uvec2();
    if size.x == 0 || size.y == 0 || size.max_element() > 8192 { return; }
    let mut image = Image::new_fill(Extent3d {width:size.x,height:size.y,depth_or_array_layers:1},
        TextureDimension::D2, &[0,0,0,0], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = models.images.add(image);
    let layer = RenderLayers::layer(LAYER);
    let root = commands.spawn((Name::new("Dialogue-owned actual Pip-Boy"), Transform::IDENTITY, Visibility::Inherited)).id();
    let mut materials = Vec::new();
    let mut screen_material = None;
    let mut blockers = Vec::new();
    let mut power_button = None;
    for (shape, part, transform, pose) in prepared {
        let mut material = shape.material.clone();
        if let Some(source) = models.materials.get(&shape.material) {
            let copy = match part {
                Some(PipBoyPart::Screen) => Some(dialogue_screen_material(source, request.image.clone())),
                _ => dialogue_accessory_material(source),
            };
            if let Some(copy) = copy {
                material = models.materials.add(copy);
                materials.push(material.clone());
                if part == Some(PipBoyPart::Screen) { screen_material = Some(material.clone()); }
            }
        }
        let is_power = shape.name.as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().contains("powerbutton"));
        if let Some(pose) = pose {
            if is_power && power_button.is_none() {
                power_button = Some(pose.clone());
            }
            if !matches!(part,Some(PipBoyPart::Screen|PipBoyPart::Glare)) { blockers.push(pose); }
        }
        commands.spawn((Name::new(format!("Dialogue Pip-Boy {}",shape.name.as_deref().unwrap_or("shape"))),
            Mesh3d(shape.mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(Mat4::from(transform)),
            bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver,
            layer.clone(), ChildOf(root)));
    }
    let camera = commands.spawn((Name::new("Dialogue Pip-Boy camera"), Camera3d::default(),
        Camera {order:-3,clear_color:ClearColorConfig::Custom(Color::NONE),..default()},
        RenderTarget::Image(ImageRenderTarget::from(image.clone())),
        Projection::Perspective(PerspectiveProjection {fov:FOV,near,far,..default()}),
        bevy::core_pipeline::tonemapping::Tonemapping::Linear, Msaa::Sample4,
        AmbientLight {color:Color::WHITE,brightness:150.0,..default()}, camera_transform,
        layer.clone(), ChildOf(root))).id();
    commands.spawn((DirectionalLight {illuminance:3000.0,shadow_maps_enabled:false,..default()},
        Transform::from_translation(camera_transform.translation).looking_at(aim,Vec3::Y),
        layer, ChildOf(root)));
    let Some(screen_material) = screen_material else {
        let image_id = image.id();
        let material_ids: Vec<_> = materials.iter().map(Handle::id).collect();
        commands.queue(move |world: &mut World| {
            if let Ok(entity) = world.get_entity_mut(root) { entity.despawn(); }
            if let Some(mut images) = world.get_resource_mut::<Assets<Image>>() { images.remove(image_id); }
            if let Some(mut mats) = world.get_resource_mut::<Assets<NifMaterial>>() {
                for id in material_ids { mats.remove(id); }
            }
        });
        return;
    };
    owned.scene = Some(Scene {root,camera,image,materials,screen_material,glass,blockers,power_button,uv,
        projection:None,rejected:None});
    info!("dialogue Pip-Boy: resolved {path}, actual NIF bind pose, independent layer{LAYER}");
}

fn camera_signature(
    scene: &Scene, request: &Request,
    cameras: &Query<(&Camera,&GlobalTransform,&RenderTarget)>, windows: &Query<&Window>,
) -> Option<ProjectionKey> {
    let (camera,transform,target) = cameras.get(scene.camera).ok()?;
    let window = windows.get(request.stamp.window).ok()?;
    let physical = Rect::from_corners(Vec2::ZERO,window.physical_size().as_vec2()).inflate(0.5);
    if !camera.is_active || !window.focused || !physical.contains(request.stamp.display.min)
        || !physical.contains(request.stamp.display.max)
        || !matches!(target,RenderTarget::Image(image) if image.handle == scene.image) { return None; }
    let logical = camera.logical_viewport_rect()?;
    Some((camera.clip_from_view(),transform.affine(),Some(logical),camera.physical_target_size()))
}

fn publish(
    mut commands: Commands, request: Option<Res<Request>>, sources: Sources, mut owned: ResMut<Owned>,
    cameras: Query<(&Camera,&GlobalTransform,&RenderTarget)>, windows: Query<&Window>,
    mut materials: ResMut<Assets<NifMaterial>>,
) {
    let Some(current) = request.as_deref() else { return; };
    let Some(expected) = owned.request.as_ref() else { return; };
    if !same(current,expected) { return; }
    if !owned.provider.as_ref().is_some_and(|p| p.matches(&sources)) {
        remove_view(&mut commands,current); return;
    }
    let Some(scene) = owned.scene.as_mut() else { return; };
    let Some(signature) = camera_signature(scene,current,&cameras,&windows) else {
        scene.projection=None; scene.rejected=None;
        remove_view(&mut commands,current); return;
    };
    if scene.projection == Some(signature) || scene.rejected == Some(signature) { return; }
    if scene.projection.take().is_some() {
        // Give the raw reader a complete frame without a projection before rebinding.
        remove_view(&mut commands,current); return;
    }
    let Ok((camera,transform,_)) = cameras.get(scene.camera) else { return; };
    for step in 0..UV_ATTEMPTS {
        let uv_size = scene.uv.size() * (0.95_f32 * 0.9_f32.powi(step));
        let uv = Rect::from_center_size(scene.uv.center(),uv_size);
        let scale = current.canvas.size()/uv.size();
        let uv_to_canvas = Affine2::from_scale_angle_translation(scale,0.0,current.canvas.min-uv.min*scale);
        let Some((map,scale)) = GlassMap::new(scene.glass.clone(),scene.blockers.clone(),camera,transform,
            current.stamp.display,current.canvas,uv_to_canvas) else { continue; };
        let btn_size = (56.0 * current.stamp.density).max(48.0);
        let margin = (8.0 * current.stamp.density).max(6.0);
        let fallback_power = Rect::from_corners(
            Vec2::new(
                (current.stamp.display.max.x - margin - btn_size).max(current.stamp.display.min.x),
                (current.stamp.display.max.y - margin - btn_size).max(current.stamp.display.min.y),
            ),
            Vec2::new(
                (current.stamp.display.max.x - margin).max(current.stamp.display.min.x + 1.0),
                (current.stamp.display.max.y - margin).max(current.stamp.display.min.y + 1.0),
            ),
        );
        let map = map.with_power_button(scene.power_button.clone(), Some(fallback_power), current.stamp.density);
        let Some(mut material) = materials.get_mut(&scene.screen_material) else { break; };
        material.base.uv_transform = Affine2::from_scale(current.target.as_vec2().recip()) * uv_to_canvas;
        let stamp = current.stamp;
        let image = scene.image.clone();
        let camera = scene.camera;
        let root = scene.root;
        let expected = (*current).clone();
        commands.queue(move |world: &mut World| {
            let live = world.get_resource::<Owned>().is_some_and(|o|
                o.scene.as_ref().is_some_and(|s| s.root == root && s.projection.is_some()));
            if live && world.get_resource::<Request>().is_some_and(|r| same(r,&expected)) {
                world.insert_resource(DeviceView {stamp,image,camera,scale,map:Arc::new(map)});
            }
        });
        scene.projection=Some(signature);
        info!("dialogue Pip-Boy: calibrated actual glass UV {uv:?}, display {:?}, conservative pixels/canvas {scale}",current.stamp.display);
        return;
    }
    scene.rejected=Some(signature);
    remove_view(&mut commands,current);
    warn!("dialogue Pip-Boy: no fully covered calibrated glass projection; no view published");
}

#[cfg(test)]
mod tests;
