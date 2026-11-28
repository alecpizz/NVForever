//! A weapon's scope while it is up (`nvf_play::aim_zoom::Scope`,
//! docs/spec-notes/weapon-scopes.md): its picture, its night vision and its sway.
//!
//! - **The picture** (§3): the weapon's MOD3 model (`interface\HUD\*.nif`) drawn by an
//!   orthographic camera of its own looking along the model's +Y, [`VIEW_HEIGHT`] model
//!   units high [hypothesis: the black mask's rectangle, 1480 × 920, is the screen at
//!   16:10], on a render layer of its own, into a picture shown full-screen under the HUD
//!   [design]. Black bars stand beyond the mask's sides on wider screens [design]. The
//!   materials are the model's own.
//! - **Night vision** (§4): a night-vision scope holds `NightScopeISFX` on the screen at its
//!   first keys in an interior or at night [documented: GECK wiki "Weapons"; hypothesis:
//!   the record, and night as sunset's end to sunrise's start].
//! - **Sway** (§5): `scopewobble.nif`'s 5 s loop of the `Camera1st` node turns the view,
//!   scaled so its widest point is half of PSp × 5.955° [documented: GECK wiki "Gun Spread
//!   Formula (New Vegas)"] × `fGunWobbleMultScope`, × `fNonAttackGunWobbleMult` while the
//!   player is not attacking [documented: the same page; hypothesis: the curve's meaning
//!   and the axes, X up and down, Z sideways]. The shots follow the view.
//! - **3D scopes** (`Scope::lens_3d`, a test version; docs/design/scope-3d-lens.md): the
//!   picture is not shown and the arms stay. A second world camera, under the player's
//!   camera, renders the zoomed world into a square image; a disc on the weapon's
//!   eyepiece (`scope_lens::LensMount`) shows it, with the MOD3 reticle drawn over it
//!   without its black mask. The camera's angle keeps the full zoom's magnification for
//!   the lens's size on screen [design].

use crate::imagespace::ScreenModifiers;
use crate::sky::SkyState;
use crate::world::WorldData;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::AmbientLight;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use fnv_nif::anim::TransformTrack;
use nvf_assets::data::GameData;
use nvf_assets::model::ModelCtx;
use nvf_play::aim_zoom::Scope;
use nvf_play::combat::Combat;
use nvf_play::player::{Player, MAX_PITCH};
use nvf_play::PlayerCamera;
use crate::scope_lens::LensMount;
use bevy::pbr::DistanceFog;
use std::sync::Arc;

/// The render layer of the scope's picture and its camera.
pub const SCOPE_LAYER: usize = 13;
/// Where the picture is put, far from the world and the other menus' models (Bevy space)
/// [design].
const PLACE: Vec3 = Vec3::new(0.0, -220_000.0, 0.0);
/// Model units across the screen's height [hypothesis, module docs].
pub const VIEW_HEIGHT: f32 = 920.0;
/// The mask's half width: black beyond it [observed: `keepScope01:0` spans ±740].
const MASK_HALF_WIDTH: f32 = 740.0;
/// The night-vision scope's image space modifier, by Editor ID [hypothesis, §4]. No DOBJ
/// place names a scope image space (default-objects.md §1), so it stays an Editor ID.
pub const NIGHT_SCOPE_IMAD: &str = "NightScopeISFX";
/// The scope's wobble [observed, §5].
pub const WOBBLE_MODEL: &str = "meshes\\characters\\weaponwobbles\\scopewobble.nif";
/// Spread units to degrees of the sway circle's diameter [documented, §5].
pub const SPREAD_TO_DEG: f32 = 5.955;
/// A shot within this many seconds counts as attacking (full sway) [hypothesis].
const ATTACK_SECONDS: f32 = 0.5;
/// 3D scopes: the mask's inner circle (model units) the reticle is cut to [observed:
/// `keepScope01:0`'s ring, weapon-scopes.md §3].
const LENS_CIRCLE: f32 = 361.0;
/// 3D scopes: the reticle's and the zoomed world's images (pixels, square) [design].
const RETICLE_PX: u32 = 512;
const LENS_WORLD_PX: u32 = 768;
/// 3D scopes: the reticle drawn this far in front of the world disc (model units).
const RETICLE_LIFT: f32 = 0.02;

/// The second world camera of a 3D scope (it takes the sun visibility too:
/// sun_visibility.rs).
#[derive(Component)]
pub(crate) struct LensCamera;

pub struct ScopeViewPlugin;

impl Plugin for ScopeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                show.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync),
                night_vision.after(nvf_play::PlaySet::Sync),
                sway.in_set(nvf_play::PlaySet::Activation).run_if(resource_exists::<Combat>),
                // After the desert haze and the aerial perspective (fog.rs FogWriters): the
                // lens copies the main camera's fog.
                lens_camera.after(show).after(crate::fog::FogWriters),
            ),
        )
        // After underwater_fog in PostUpdate so the 3D scope lens receives the current
        // frame's underwater fog or freshly restored weather/interior fog.
        .add_systems(PostUpdate, sync_lens_fog.after(crate::underwater::underwater_fog));
    }
}

/// The game path of a MOD3 model (stored without `meshes\`).
pub fn model_path(mod3: &str) -> String {
    let p = mod3.replace('/', "\\");
    if p.to_ascii_lowercase().starts_with("meshes\\") {
        p
    } else {
        format!("meshes\\{p}")
    }
}

/// The time of day counts as night: from the end of sunset to the start of sunrise
/// (hours) [hypothesis, §4].
pub fn is_night(hour: f32, sunrise_begin: f32, sunset_end: f32) -> bool {
    let h = hour.rem_euclid(24.0);
    h >= sunset_end || h < sunrise_begin
}

/// The picture as drawn: its entities, the model and the window size it was built for.
#[derive(Default)]
struct Drawn {
    entities: Vec<Entity>,
    model: Option<String>,
    lens: bool,
    size: UVec2,
    warned: Option<String>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show(
    mut commands: Commands,
    scope: Res<Scope>,
    mut models: ModelCtx,
    mut plain: ResMut<Assets<StandardMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
    (mounts, main_cam): (Query<(Entity, &LensMount)>, Query<Entity, With<PlayerCamera>>),
    mut drawn: Local<Drawn>,
) {
    let size = window.single().map(|w| UVec2::new(w.physical_width().max(1), w.physical_height().max(1))).unwrap_or(UVec2::new(1280, 720));
    let want = scope.model.as_deref().filter(|_| scope.on).map(model_path);
    let lens = scope.lens_3d;
    if drawn.model.is_some() && (drawn.model != want || drawn.lens != lens || (!lens && drawn.size != size)) {
        for e in drawn.entities.drain(..) {
            commands.entity(e).despawn();
        }
        drawn.model = None;
    }
    let Some(path) = want else { return };
    if drawn.model.is_some() {
        return;
    }
    let Some(model) = models.model(&path) else {
        if drawn.warned.as_deref() != Some(&path) {
            warn!("scope view: {path} not loaded; the scope is shown without its picture");
            drawn.warned = Some(path);
        }
        return;
    };
    if lens {
        let (Some((mount, m)), Ok(cam)) = (mounts.iter().next(), main_cam.single()) else { return };
        drawn.entities = draw_lens(&mut commands, &model, &mut models, mount, m.radius, cam);
        info!("scope view: {path} in the lens (3D scope), radius {:.2}, reticle cut to {LENS_CIRCLE} units", m.radius);
        drawn.model = Some(path);
        drawn.lens = true;
        return;
    }
    let mut image = Image::new_fill(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = models.images.add(image);
    let layer = RenderLayers::layer(SCOPE_LAYER);
    // Game space (Z up) into Bevy's (Y up), as the world root does.
    let place = Affine3A::from_translation(PLACE) * Affine3A::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let to = |p: Vec3| place.transform_point3(p);
    let mut entities = Vec::new();
    for s in &model.shapes {
        let world = place * model.root * s.transform;
        let name = s.name.as_deref().unwrap_or("");
        entities.push(commands.spawn((Name::new(format!("Scope {name}")), Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), Transform::from_matrix(Mat4::from(world)), layer.clone())).id());
    }
    // Black beyond the mask's sides, for screens wider than 16:10.
    let bar = models.meshes.add(Rectangle::new(4.0 * VIEW_HEIGHT, 2.0 * VIEW_HEIGHT));
    let black = plain.add(StandardMaterial { base_color: Color::BLACK, unlit: true, ..default() });
    for side in [-1.0f32, 1.0] {
        let at = to(Vec3::new(side * (MASK_HALF_WIDTH + 2.0 * VIEW_HEIGHT - 1.0), 0.0, 0.0));
        entities.push(commands.spawn((Name::new("Scope side bar"), Mesh3d(bar.clone()), MeshMaterial3d(black.clone()), Transform::from_translation(at), layer.clone())).id());
    }
    let eye = Vec3::new(0.0, -1000.0, 0.0);
    entities.push(
        commands
            .spawn((
                Name::new("Scope camera"),
                Camera3d::default(),
                Camera { order: -5, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
                RenderTarget::Image(ImageRenderTarget::from(image.clone())),
                Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::FixedVertical { viewport_height: VIEW_HEIGHT }, near: 0.0, far: 3000.0, ..OrthographicProjection::default_3d() }),
                Tonemapping::Linear,
                Msaa::Sample4,
                AmbientLight { color: Color::WHITE, brightness: 500.0, ..default() },
                Transform::from_translation(to(eye)).looking_at(to(Vec3::ZERO), Vec3::Y),
                layer.clone(),
            ))
            .id(),
    );
    // Under the HUD (whose nodes sit at the default z index), over the world.
    entities.push(
        commands
            .spawn((
                Name::new("Scope picture"),
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                ImageNode::new(image),
                GlobalZIndex(-1),
                Pickable::IGNORE,
            ))
            .id(),
    );
    info!("scope view: {path} drawn ({} shapes), {VIEW_HEIGHT} units high", model.shapes.len());
    drawn.entities = entities;
    drawn.model = Some(path);
    drawn.lens = false;
    drawn.size = size;
}

/// A square image to render into: `hdr` linear (the world, tonemapped by the main
/// camera when drawn on the lens), else sRGB with alpha (the reticle).
fn target_image(images: &mut Assets<Image>, px: u32, hdr: bool) -> Handle<Image> {
    let format = if hdr { TextureFormat::Rgba16Float } else { TextureFormat::Bgra8UnormSrgb };
    let mut image = Image::new_uninit(Extent3d { width: px, height: px, depth_or_array_layers: 1 }, TextureDimension::D2, format, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    images.add(image)
}

/// The 3D scope: the reticle (the MOD3 model without its mask) and the zoomed world, each
/// rendered into an image, shown on two discs at the eyepiece `mount`.
fn draw_lens(commands: &mut Commands, model: &nvf_assets::model::PreparedModel, models: &mut ModelCtx, mount: Entity, radius: f32, cam: Entity) -> Vec<Entity> {
    let mut entities = Vec::new();
    let reticle = target_image(&mut models.images, RETICLE_PX, false);
    let layer = RenderLayers::layer(SCOPE_LAYER);
    let place = Affine3A::from_translation(PLACE) * Affine3A::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    for s in &model.shapes {
        let name = s.name.as_deref().unwrap_or("").to_ascii_lowercase();
        // The reticle only: not the black mask (vanilla `keepScope01:0`; `MaskRing*` in
        // the other layouts), nor the tint ring, the glare (`Additive01:5`) or the inner
        // shading, which over the lens drew as a dark veil (drawn into a clear image they
        // keep their alpha) [observed: scope-3d-lens-interior, the lens much darker than
        // the door behind it until they were left out].
        let veil = ["keepscope", "maskring", "tintring", "additive", "inner lens shading"];
        if veil.iter().any(|v| name.starts_with(v)) {
            continue;
        }
        let world = place * model.root * s.transform;
        entities.push(commands.spawn((Name::new(format!("Scope lens reticle {name}")), Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), Transform::from_matrix(Mat4::from(world)), layer.clone())).id());
    }
    let to = |p: Vec3| place.transform_point3(p);
    entities.push(
        commands
            .spawn((
                Name::new("Scope lens reticle camera"),
                Camera3d::default(),
                Camera { order: -5, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
                RenderTarget::Image(ImageRenderTarget::from(reticle.clone())),
                Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::FixedVertical { viewport_height: 2.0 * LENS_CIRCLE }, near: 0.0, far: 3000.0, ..OrthographicProjection::default_3d() }),
                Tonemapping::Linear,
                Msaa::Sample4,
                AmbientLight { color: Color::WHITE, brightness: 500.0, ..default() },
                Transform::from_translation(to(Vec3::new(0.0, -1000.0, 0.0))).looking_at(to(Vec3::ZERO), Vec3::Y),
                layer,
            ))
            .id(),
    );
    // The zoomed world: the main camera's view without the arms and body and the scope's
    // own layer; the impact decals too (they need a depth prepass, which it has). Its own
    // grade holds only what belongs in the lens (a night-vision scope's modifier).
    let world_image = target_image(&mut models.images, LENS_WORLD_PX, true);
    let lens_cam = commands
        .spawn((
            Name::new("Scope lens camera"),
            LensCamera,
            Camera3d::default(),
            Camera { order: -4, ..default() },
            RenderTarget::Image(ImageRenderTarget::from(world_image.clone())),
            bevy::camera::Hdr,
            Tonemapping::Linear,
            Msaa::Sample4,
            Projection::Perspective(PerspectiveProjection { fov: 0.1, near: 5.0, far: crate::sky::OUTDOOR_FAR, aspect_ratio: 1.0, ..default() }),
            crate::sky::initial_fog(),
            bevy::core_pipeline::prepass::DepthPrepass,
            RenderLayers::from_layers(&[0, crate::reflection::WATER_LAYER, crate::decals::DECAL_LAYER, crate::volumetric_clouds::FLAT_CLOUD_LAYER]),
            (crate::imagespace::ImageSpaceGrade::default(), crate::imagespace::LensGrade),
            Transform::IDENTITY,
        ))
        .id();
    commands.entity(cam).add_child(lens_cam);
    entities.push(lens_cam);
    let disc = models.meshes.add(Circle::new(radius));
    let arms = RenderLayers::layer(crate::sky::ARMS_LAYER);
    // NIF materials drawn as first-person shapes, as the rifle and its scope around them
    // are (docs/design/first-person-depth.md): at world depth the squeezed scope tube hid
    // the discs inside it.
    let first_person = |base: StandardMaterial| {
        let mut m = nvf_assets::nif_material::nif_material(base);
        m.extension.first_person = true;
        m
    };
    let world_mat = models.materials.add(first_person(StandardMaterial { base_color_texture: Some(world_image), unlit: true, fog_enabled: false, ..default() }));
    let reticle_mat = models.materials.add(first_person(StandardMaterial { base_color_texture: Some(reticle), unlit: true, fog_enabled: false, alpha_mode: AlphaMode::Blend, ..default() }));
    for (name, mat, z) in [("Scope lens world", world_mat, 0.0), ("Scope lens reticle", reticle_mat, RETICLE_LIFT)] {
        let e = commands.spawn((Name::new(name), Mesh3d(disc.clone()), MeshMaterial3d(mat), Transform::from_xyz(0.0, 0.0, z), arms.clone(), bevy::light::NotShadowCaster)).id();
        commands.entity(mount).add_child(e);
        entities.push(e);
    }
    entities
}

/// Keep the 3D scope's camera on the main view's fog and far plane, and its angle such
/// that the lens magnifies as the full zoom does: the lens's angular radius on screen
/// (tan a) over the zoom's magnification tan(base/2) / tan(sight/2).
#[allow(clippy::type_complexity)]
fn lens_camera(
    zoom: Option<Res<nvf_play::aim_zoom::AimZoom>>,
    (player, sway): (Option<Res<Player>>, Res<nvf_play::aim_zoom::LensSway>),
    main: Query<(&GlobalTransform, &Projection, &DistanceFog), (With<PlayerCamera>, Without<LensCamera>)>,
    mut lens: Query<(&mut Projection, &mut DistanceFog, &mut Transform), With<LensCamera>>,
    mounts: Query<(&GlobalTransform, &LensMount)>,
    mut told: Local<Option<i32>>,
) {
    let Ok((mut proj, mut fog, mut turn)) = lens.single_mut() else {
        *told = None;
        return;
    };
    let (Ok((cam, main_proj, main_fog)), Some((mount, m)), Some(z)) = (main.single(), mounts.iter().next(), zoom) else { return };
    let (Some(base), Some(sight), Projection::Perspective(mp)) = (z.base(), z.sight, main_proj) else { return };
    let centre = mount.translation();
    let edge = mount.transform_point(Vec3::X * m.radius);
    let tan_a = (edge - centre).length() / (centre - cam.translation()).length().max(1e-3);
    let magnification = (base / 2.0).tan() / (sight / 2.0).tan().max(1e-4);
    let fov = 2.0 * (tan_a / magnification.max(1e-3)).atan();
    if let Projection::Perspective(p) = &mut *proj {
        if (p.fov - fov).abs() > 1e-5 || p.far != mp.far {
            p.fov = fov;
            p.far = mp.far;
            p.aspect_ratio = 1.0;
        }
    }
    *fog = main_fog.clone();
    // The sway turns the lens's view with the gun, off the main view's (§5): the turn
    // from the view's direction to the swayed one, in the main camera's frame.
    let local = match player.as_deref() {
        Some(p) if *sway != nvf_play::aim_zoom::LensSway::default() => {
            let dir = |d: [f32; 3]| nvf_play::space::to_bevy(d) - nvf_play::space::to_bevy([0.0; 3]);
            let (from, to) = (dir(nvf_play::space::look_dir(p.heading, p.pitch)), dir(sway.look_dir(p.heading, p.pitch)));
            let r = cam.compute_transform().rotation;
            r.inverse() * Quat::from_rotation_arc(from.normalize_or(Vec3::NEG_Z), to.normalize_or(Vec3::NEG_Z)) * r
        }
        _ => Quat::IDENTITY,
    };
    if turn.rotation != local {
        turn.rotation = local;
    }
    let deg = (fov.to_degrees() * 10.0).round() as i32;
    if told.is_none_or(|t| (t - deg).abs() > 5) {
        *told = Some(deg);
        info!("scope lens camera: FOV {:.1}°, the lens {:.1}° across on screen, magnification {magnification:.2}", fov.to_degrees(), 2.0 * tan_a.atan().to_degrees());
    }
}

/// Keep the 3D scope lens's fog synchronized with the main camera after `underwater_fog`
/// runs in `PostUpdate`, avoiding 1-frame stale underwater/above-water fog in the lens.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_lens_fog(
    main: Query<&DistanceFog, (crate::fly::WorldCamera, Without<LensCamera>)>,
    mut lens: Query<&mut DistanceFog, With<LensCamera>>,
) {
    let Ok(mut fog) = lens.single_mut() else { return };
    let Ok(main_fog) = main.single() else { return };
    *fog = main_fog.clone();
}

/// Hold the night-vision scope's modifier on the screen while it gives night vision.
fn night_vision(scope: Res<Scope>, sky: Option<Res<SkyState>>, world: Option<Res<WorldData>>, mut mods: ResMut<ScreenModifiers>, mut state: Local<(Option<Option<u32>>, bool, bool)>, tweaks: Option<Res<nvf_play::tweaks::TweakSettings>>) {
    // The player's Night Vision Toggle tweak's By Day Too [design: docs/design/tweaks.md].
    let by_day = tweaks.as_ref().is_some_and(|t| t.night_vision_toggle && t.night_vision_by_day);
    let night = by_day || sky.as_deref().is_some_and(|s| s.interior.is_some() || is_night(s.hour, s.climate.sunrise.0, s.climate.sunset.1));
    let on = scope.on && scope.night_vision && night;
    // A 3D scope gives its night vision in the lens only [design].
    let lens = on && scope.lens_3d;
    if (on, lens) == (state.1, state.2) {
        return;
    }
    (state.1, state.2) = (on, lens);
    let Some(world) = world else { return };
    let form = *state.0.get_or_insert_with(|| fnv_world::image_modifier::find_image_modifier(world.lo, world.schema, NIGHT_SCOPE_IMAD));
    match form {
        Some(f) => {
            mods.hold(f, on && !lens);
            mods.hold_lens(f, lens);
            info!("scope: night vision {NIGHT_SCOPE_IMAD} {}", if lens { "on in the lens" } else if on { "on" } else { "off" });
        }
        None if on => warn!("scope: no image space modifier {NIGHT_SCOPE_IMAD}; the night-vision scope shows no night vision"),
        None => {}
    }
}

/// The wobble's curves: the track, its span (seconds) and its widest point (radians).
struct Wobble {
    track: Arc<TransformTrack>,
    start: f32,
    stop: f32,
    peak: f32,
}

impl Wobble {
    fn load(data: &GameData) -> Option<Wobble> {
        let bytes = data.read(WOBBLE_MODEL)?;
        let file = fnv_nif::NifFile::parse(&bytes, Some(&data.schema)).ok()?;
        let dec = fnv_nif::scene::Decoded::new(&file, &bytes, &data.schema);
        let (_, track, cycle) = nvf_assets::model::free_transform_tracks(&file, &dec).into_iter().find(|(n, ..)| n.eq_ignore_ascii_case("Camera1st"))?;
        let mut w = Wobble { track, start: cycle.start, stop: cycle.stop, peak: 0.0 };
        if !(w.start.is_finite() && w.stop.is_finite()) {
            return None;
        }
        let widths: Vec<f32> = (0..=200).map(|k| w.angles(k as f32 / 200.0)).map(|[x, z]| x.hypot(z)).collect();
        // A broken file's NaN or infinite turn would reach the player's view: no sway.
        if !widths.iter().all(|v| v.is_finite()) {
            return None;
        }
        w.peak = widths.into_iter().fold(0.0, f32::max);
        (w.peak > 0.0).then_some(w)
    }

    /// The node's turn about X and Z (radians) at `share` (0–1) of the loop.
    fn angles(&self, share: f32) -> [f32; 2] {
        let t = self.start + (self.stop - self.start) * share.rem_euclid(1.0);
        if let Some(e) = &self.track.euler {
            let at = |k: usize| e[k].sample(t).map_or(0.0, |v| v[0]);
            return [at(0), at(2)];
        }
        let [w, x, y, z] = self.track.rotation_at(t).unwrap_or([1.0, 0.0, 0.0, 0.0]);
        let (ex, _, ez) = Quat::from_xyzw(x, y, z, w).to_euler(EulerRot::XYZ);
        [ex, ez]
    }

    /// The view's turn (yaw, pitch; degrees) `t` seconds into the loop, scaled so the
    /// widest point is `radius` degrees.
    fn turn(&self, t: f32, radius: f32) -> [f32; 2] {
        let span = (self.stop - self.start).max(0.01);
        let [x, z] = self.angles(t / span);
        [z / self.peak * radius, x / self.peak * radius]
    }
}

#[derive(Default)]
struct SwayState {
    wobble: Option<Option<Wobble>>,
    t: f32,
    last: [f32; 2],
    told: bool,
}

/// Sway the view while the scope is up (§5).
#[allow(clippy::too_many_arguments)]
fn sway(
    time: Res<Time>,
    scope: Res<Scope>,
    combat: Res<Combat>,
    sim: Option<Res<nvf_play::sim::SimState>>,
    data: Option<Res<GameData>>,
    mut player: ResMut<Player>,
    mut lens_sway: ResMut<nvf_play::aim_zoom::LensSway>,
    mut st: Local<SwayState>,
    tweaks: Option<Res<nvf_play::tweaks::TweakSettings>>,
) {
    let want = if scope.on {
        if st.wobble.is_none() {
            // A panic reading a broken file counts as no wobble.
            let w = data.as_deref().and_then(|d| nvf_assets::guard::guarded(|| Ok(Wobble::load(d))).unwrap_or_else(|e| {
                warn!("scope sway: {WOBBLE_MODEL} not read: {e}");
                None
            }));
            if w.is_none() {
                warn!("scope sway: {WOBBLE_MODEL} has no Camera1st rotation; scopes do not sway");
            }
            st.wobble = Some(w);
        }
        let setting = |name: &str, or: f32| sim.as_deref().and_then(|s| s.forms.setting_f32(name)).unwrap_or(or);
        let spread = combat.player_spread.unwrap_or(0.0);
        let attacking = combat.since_last_shot() < ATTACK_SECONDS;
        let radius = spread * SPREAD_TO_DEG / 2.0 * setting("fGunWobbleMultScope", 1.0) * if attacking { 1.0 } else { setting("fNonAttackGunWobbleMult", 0.5) };
        // The player's Hold Breath tweak steadies it (docs/design/tweaks.md).
        let held = nvf_play::tweaks::BREATH_HELD.load(std::sync::atomic::Ordering::Relaxed);
        let radius = if held { radius * tweaks.as_ref().map_or(1.0, |t| t.breath_sway) } else { radius };
        if !st.told {
            st.told = true;
            info!("scope sway: PSp {spread:.3}, radius {radius:.2}°");
        }
        st.t += time.delta_secs();
        let t = st.t;
        st.wobble.as_ref().and_then(Option::as_ref).map_or([0.0, 0.0], |w| w.turn(t, radius))
    } else {
        st.t = 0.0;
        st.told = false;
        [0.0, 0.0]
    };
    // A 3D scope sways the gun and its lens (`LensSway`); the picture sways the view.
    let (view, lens) = if scope.lens_3d { ([0.0, 0.0], want) } else { (want, [0.0, 0.0]) };
    let turn = nvf_play::aim_zoom::LensSway { yaw: lens[0].to_radians(), pitch: lens[1].to_radians() };
    if *lens_sway != turn {
        *lens_sway = turn;
    }
    if view == st.last {
        return;
    }
    player.heading += (view[0] - st.last[0]).to_radians();
    player.pitch = (player.pitch + (view[1] - st.last[1]).to_radians()).clamp(-MAX_PITCH, MAX_PITCH);
    st.last = view;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_runs_from_sunset_end_to_sunrise_begin() {
        assert!(is_night(23.0, 6.0, 20.0));
        assert!(is_night(2.0, 6.0, 20.0));
        assert!(is_night(20.0, 6.0, 20.0));
        assert!(!is_night(6.0, 6.0, 20.0));
        assert!(!is_night(12.0, 6.0, 20.0));
        assert!(is_night(26.0, 6.0, 20.0));
    }

    #[test]
    fn mod3_paths_gain_the_meshes_folder_once() {
        assert_eq!(model_path("interface\\HUD\\mildotscope.NIF"), "meshes\\interface\\HUD\\mildotscope.NIF");
        assert_eq!(model_path("Meshes\\interface\\hud\\x.nif"), "Meshes\\interface\\hud\\x.nif");
    }
}
