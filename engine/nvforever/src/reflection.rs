//! World reflections in water: a second camera, the main camera mirrored in the water
//! plane nearest the camera, renders the scene above that plane (an oblique near clip
//! plane cuts away everything below it) into a texture; the water shader samples it at
//! the fragment's screen position, offset by the wave normal. The same technique as
//! Bevy's `mirror` example. Water itself lives on [`WATER_LAYER`], which the reflection
//! camera does not draw.
//!
//! One plane at a time [hypothesis: enough for the Mojave, where lakes do not overlap
//! in height on screen]: water at another height reflects only the sky (water.wesl).
//!
//! Only when water is seen: the camera renders while water lies within [`SEARCH`] cells
//! and some water at the reflected height lay inside the main camera's view within the
//! last [`KEEP_UNSEEN`] seconds ([`water_on_screen`]: the wet part of each cell and placed
//! water objects, clipped against the view volume in the same frame). It draws the whole
//! scene again, and Bevy gives every active camera its own set of the sun's shadow
//! cascades, so it cost about as much as the main view's shadows: in Primm by day, with
//! water two cells off and out of sight, 19.5 ms a frame with it against 12.5 without
//! (RTX 4070, 1080p, fine shadows, 8x MSAA; 2026-10-30) [observed]. Bevy's own view
//! visibility of the water meshes would not do: its box test passes a 4,096-unit plane
//! that is nowhere in view, and every cell's plane, buried ones included, has one. Water
//! inside the view but hidden behind a hill or a building still counts [known gap].
//! Water that is not drawn does not count (#1059): a placed water object not visible (a
//! disabled reference, a hidden model) and any water while the console hides water.
//!
//! The CPU view test cannot see opaque occluders (#1060). For the current candidate
//! plane, the water shader counts unoccluded fragments on a rotating 4×4 sample grid.
//! The render world resets and tags each frame's count before drawing; asynchronous
//! readbacks carry the probe identity, source frame and source time, not delivery time.
//! Delayed results for another plane and out-of-order results cannot drive the camera.
//!
//! Visible samples turn the camera on. To turn it off on GPU evidence requires a
//! sustained run of zero samples, not a single flat interval. Missing results are
//! not zeros. The CPU still turns it off after [`KEEP_UNSEEN`] out of view, or at
//! once with no eligible plane. The mirror texture is drawn before the main camera
//! (order -1), including the frame in which the material starts using it.
//!
//! Switched off by the `water-mirror` setting (graphics.rs `Graphics::water_mirror`;
//! #1380): water then reflects the sky, and the screen where screen-space reflections are
//! on.
//! At Hoover Dam by day (Lake Mead in view) the camera costs about 0.5 ms a frame, where
//! the CPU sets the frame: 7.6 ms with it, 7.1 without [observed 2026-11-02, #1380]. Its
//! own set of the sun's cascades stays: Bevy's renderer needs cascades for every view the
//! sun lights, and emptying their caster lists saved 0.12 ms of shadow-pass GPU time and
//! nothing of the frame there, so the reflected world keeps its shadows. While the sun's
//! cascades are drawn on a schedule the camera is given the main camera's cascades
//! (shadow_rate.rs `hold_cascades`): both views share the shadow-map layers, so it draws
//! and reads the same maps and held cascades stay right for it (#2674).

use crate::water::{DistantWater, WaterCache, WaterMaterial};
use crate::world::{bevy_to_game, Streamer, WorldData};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::SimulationLightSystems;
use bevy::math::reflection_matrix;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::gpu_readback::ReadbackComplete;
#[cfg(any(not(target_os = "android"), test))]
use bevy::render::gpu_readback::Readback;
#[cfg(any(target_os = "android", test))]
use bevy::render::gpu_readback::ReadbackOnce;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderQueue;
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::transform::TransformSystems;
use bevy::window::{PrimaryWindow, WindowResized};
use fnv_world::cell_of;
use nvf_play::RefVisual;
use bevy::camera::primitives::Aabb;

mod clouds;
pub(crate) use clouds::ReflectedClouds;

/// Render layer of water surfaces: seen by the main camera, not by the reflection camera
/// (which would otherwise sample the texture it renders into).
pub const WATER_LAYER: usize = 1;

/// The reflection texture's size relative to the window.
const SCALE: u32 = 2;

/// Cells (in grid steps from the camera's cell) searched for the water to reflect in.
const SEARCH: i32 = 2;

/// Seconds the camera keeps rendering after water was last inside the view, so turning
/// along the edge of a lake does not switch it on and off [engineering choice].
const KEEP_UNSEEN: f32 = 1.0;

/// Real seconds of consecutive empty GPU frames required to switch off an active
/// mirror. Short gaps in coverage should not switch the water between mirror and SSR.
const GPU_HIDDEN_HOLD: f32 = 4.0;
/// Also cover a full cycle of the shader's 4×4 sampling grid.
const SAMPLE_PHASES: u32 = 16;
/// Ignore stale samples, and use the CPU view test alone if a probe never answers.
const READBACK_WAIT: f32 = 2.0;

/// Below the water plane by this much, so the water surface itself is clipped away.
const CLIP_MARGIN: f32 = 1.0;

#[derive(Component)]
pub struct ReflectionCamera;

#[derive(Resource)]
pub struct Reflection {
    pub image: Handle<Image>,
}

pub struct ReflectionPlugin;

impl Plugin for ReflectionPlugin {
    fn build(&self, app: &mut App) {
        clouds::install(app);
        app.init_resource::<WaterPixels>().init_resource::<WaterProbe>().init_resource::<ReflectionGate>().add_systems(Startup, setup_reflection).add_systems(
            PostUpdate,
            (resize_reflection, read_water_pixels, update_reflection)
                .chain()
                .after(TransformSystems::Propagate)
                .after(crate::underwater::underwater_fog)
                .before(VisibilitySystems::UpdateFrusta)
                // Cascades are built only for active cameras: switch on before that, or
                // the renderer finds an active view without sun shadow cascades.
                .before(SimulationLightSystems::UpdateDirectionalLightCascades),
        );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<WaterProbe>()
                .add_systems(ExtractSchedule, extract_water_probe)
                .add_systems(Render, reset_water_pixels.in_set(RenderSystems::PrepareResources));
        }
    }
}

fn texture_size(window: &Window) -> Extent3d {
    Extent3d {
        width: (window.physical_width() / SCALE).max(1),
        height: (window.physical_height() / SCALE).max(1),
        depth_or_array_layers: 1,
    }
}

fn setup_reflection(mut commands: Commands, mut images: ResMut<Assets<Image>>, window: Query<&Window, With<PrimaryWindow>>) {
    let size = window.single().map(texture_size).unwrap_or(Extent3d { width: 640, height: 400, depth_or_array_layers: 1 });
    // Linear HDR, not tonemapped: the water shader mixes it with lit colours and
    // tonemaps the result.
    let mut image = Image::new_uninit(
        size,
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    commands.spawn((
        Name::new("Reflection camera"),
        ReflectionCamera,
        Camera3d::default(),
        Camera { order: -1, invert_culling: true, is_active: false, ..default() },
        RenderTarget::Image(image.clone().into()),
        Hdr,
        Tonemapping::Linear,
        Msaa::Off,
        Projection::Perspective(PerspectiveProjection::default()),
        crate::sky::initial_fog(),
        // The world without the water; the player's first-person arms and seen body as
        // before they had layers of their own. Keep the flat sky until the mirror's
        // own volume has rendered and the mirror can draw its dome.
        RenderLayers::from_layers(&[0, crate::sky::ARMS_LAYER, crate::sky::BODY_LAYER, crate::volumetric_clouds::FLAT_CLOUD_LAYER, clouds::MIRROR_CLOUD_LAYER]),
    ));
    commands.insert_resource(Reflection { image });
}

fn resize_reflection(
    mut resized: MessageReader<WindowResized>,
    window: Query<&Window, With<PrimaryWindow>>,
    reflection: Option<Res<Reflection>>,
    mut images: ResMut<Assets<Image>>,
) {
    if resized.read().last().is_none() {
        return;
    }
    let (Ok(window), Some(r)) = (window.single(), reflection) else { return };
    let size = texture_size(window);
    if let Some(mut img) = images.get_mut(&r.image) {
        if img.texture_descriptor.size != size {
            img.resize(size);
        }
    }
}

/// Identifies the current visibility request. A new view candidate starts a new probe
/// even if it has the same height as a former one (e.g. after an interior transition).
#[derive(Resource, Default, Clone, Copy)]
struct WaterProbe {
    target: Option<(u32, f32)>,
    buffer: Option<AssetId<ShaderBuffer>>,
    generation: u32,
    frame: u32,
    time: f32,
}

impl WaterProbe {
    fn seen_plane(&self) -> Vec4 {
        self.target.map_or(Vec4::ZERO, |(_, height)| {
            Vec4::new(height, 1.0, (self.generation & 0xffff) as f32, (self.generation >> 16) as f32)
        })
    }

    fn sample(&self, count: u32) -> WaterSample {
        WaterSample { count, generation: self.generation, frame: self.frame, time: self.time }
    }
}

fn extract_water_probe(source: Extract<Res<WaterProbe>>, mut probe: ResMut<WaterProbe>) {
    *probe = **source;
}

/// WGPU uploads this reset before the frame's draw commands execute. Bevy copies the
/// buffer for readback after the render graph. Never reset the asset itself: the
/// materials keep the same GPU buffer bound across frames.
fn reset_water_pixels(probe: Res<WaterProbe>, buffers: Res<RenderAssets<GpuShaderBuffer>>, queue: Res<RenderQueue>) {
    if let Some(buffer) = probe.buffer.and_then(|id| buffers.get(id)).filter(|_| probe.target.is_some()) {
        queue.write_buffer(&buffer.buffer, 0, &probe.sample(0).bytes());
    }
}

/// GPU buffer ABI (four successive u32s, see `water.wesl` and `water_material`).
#[derive(Debug, Clone, Copy)]
struct WaterSample {
    count: u32,
    generation: u32,
    frame: u32,
    time: f32,
}

impl WaterSample {
    fn bytes(self) -> [u8; 16] {
        let words = [self.count, self.generation, self.frame, self.time.to_bits()];
        std::array::from_fn(|i| words[i / 4].to_le_bytes()[i % 4])
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let &[a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p] = bytes else { return None };
        Some(Self {
            count: u32::from_le_bytes([a, b, c, d]),
            generation: u32::from_le_bytes([e, f, g, h]),
            frame: u32::from_le_bytes([i, j, k, l]),
            time: f32::from_le_bytes([m, n, o, p]),
        })
    }
}

#[derive(Resource, Default)]
struct WaterPixels {
    #[cfg(any(not(target_os = "android"), test))]
    requested: Option<(AssetId<ShaderBuffer>, Entity)>,
    #[cfg(any(target_os = "android", test))]
    pending: Option<(Entity, AssetId<ShaderBuffer>)>,
    #[cfg(any(target_os = "android", test))]
    counter: Option<AssetId<ShaderBuffer>>,
    /// Start of the current probe and the newest result for it.
    started: f32,
    last: Option<WaterSample>,
    /// A positive result survives a batch that also contains newer empty frames.
    last_drawn: Option<f32>,
    /// Frame and source time of the first zero in an uninterrupted run.
    empty_since: Option<(u32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuWater {
    Drawn,
    Hidden,
    Unknown,
    Unavailable,
}

impl WaterPixels {
    fn read(&mut self, sample: WaterSample, probe: &WaterProbe, now: f32) {
        if probe.target.is_none()
            || sample.generation != probe.generation
            || !(0.0..=READBACK_WAIT).contains(&(now - sample.time))
        {
            return;
        }
        let consecutive = if let Some(last) = self.last {
            let steps = sample.frame.wrapping_sub(last.frame);
            if steps == 0 || steps >= 1 << 31 || sample.time < last.time {
                return;
            }
            steps == 1 && sample.time - last.time <= READBACK_WAIT
        } else {
            false
        };
        // A newer zero does not erase a sighting, but a new probe does.
        if sample.count != 0 {
            self.last_drawn = Some(sample.time);
        }
        self.empty_since = if sample.count != 0 {
            None
        } else if consecutive {
            self.empty_since.or(Some((sample.frame, sample.time)))
        } else {
            Some((sample.frame, sample.time))
        };
        self.last = Some(sample);
    }

    fn state(&self, now: f32) -> GpuWater {
        let Some(last) = self.last else {
            return if now - self.started >= READBACK_WAIT { GpuWater::Unavailable } else { GpuWater::Unknown };
        };
        if now - last.time > READBACK_WAIT {
            return GpuWater::Unknown;
        }
        if self.last_drawn.is_some_and(|time| now - time <= READBACK_WAIT) {
            return GpuWater::Drawn;
        }
        if self.empty_since.is_some_and(|(frame, time)| {
            last.frame.wrapping_sub(frame) >= SAMPLE_PHASES - 1 && last.time - time >= GPU_HIDDEN_HOLD
        }) {
            GpuWater::Hidden
        } else {
            GpuWater::Unknown
        }
    }
}

/// Follow the shared counter asset. Old observers cannot deliver into a new probe.
fn read_water_pixels(mut commands: Commands, cache: Res<WaterCache>, mut px: ResMut<WaterPixels>) {
    #[cfg(target_os = "android")]
    {
        if let Some(counter) = cache.seen_counter() {
            request_android_water_pixels(&mut commands, counter, &mut px);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let counter = cache.seen_counter();
        if px.requested.map(|(id, _)| id) == counter.map(Handle::id) {
            return;
        }
        if let Some((_, entity)) = px.requested.take() {
            commands.entity(entity).despawn();
        }
        let Some(counter) = counter else { return };
        let entity = commands.spawn((Name::new("Water pixel readback"), Readback::buffer(counter.clone()))).observe(
            |ev: On<ReadbackComplete>, probe: Res<WaterProbe>, mut px: ResMut<WaterPixels>, time: Res<Time<Real>>| {
                if let Some(sample) = WaterSample::decode(&ev.data) {
                    px.read(sample, &probe, time.elapsed_secs());
                }
            },
        ).id();
        px.requested = Some((counter.id(), entity));
    }
}

#[cfg(any(target_os = "android", test))]
fn request_android_water_pixels(commands: &mut Commands, counter: &Handle<ShaderBuffer>, px: &mut WaterPixels) {
    if px.counter != Some(counter.id()) {
        px.counter = Some(counter.id());
        px.last = None;
        px.last_drawn = None;
        px.empty_since = None;
    }
    if px.pending.is_some() {
        return;
    }
    let entity = commands
        .spawn((Name::new("Water pixel readback"), ReadbackOnce::buffer(counter.clone())))
        .observe(complete_android_water_pixels)
        .id();
    px.pending = Some((entity, counter.id()));
}

#[cfg(any(target_os = "android", test))]
fn complete_android_water_pixels(
    ev: On<ReadbackComplete>,
    mut commands: Commands,
    probe: Res<WaterProbe>,
    mut px: ResMut<WaterPixels>,
    time: Res<Time<Real>>,
) {
    let Some((entity, counter)) = px.pending.filter(|(entity, _)| *entity == ev.entity) else {
        commands.entity(ev.entity).despawn();
        return;
    };
    if px.counter == Some(counter) {
        if let Some(sample) = WaterSample::decode(&ev.data) {
            px.read(sample, &probe, time.elapsed_secs());
        }
    }
    px.pending = None;
    commands.entity(entity).despawn();
}

/// CPU retention is independent of GPU coverage. Track the selected world and height
/// even while out of view so the grace period never holds an unrelated plane on.
#[derive(Resource, Default)]
struct ReflectionGate {
    plane: Option<(u32, f32)>,
    last_in_view: Option<f32>,
}

/// Placed water objects (models of a placed reference with the water material): where
/// they are, their bounds and whether they are drawn.
type PlacedWater<'w, 's> = Query<
    'w,
    's,
    (&'static GlobalTransform, &'static Aabb, &'static RefVisual, &'static InheritedVisibility),
    (With<MeshMaterial3d<WaterMaterial>>, Without<ReflectionCamera>),
>;

/// What put water on screen (module notes, "Only when water is seen").
#[derive(Debug, Clone, Copy, PartialEq)]
enum SeenWater {
    /// A cell's water surface (its grid position).
    Cell((i32, i32)),
    /// A placed water object (its reference's FormID).
    Placed(u32),
}

/// Height tolerance (game units) for water counting as at the reflected plane's height.
const SAME_HEIGHT: f32 = 1.0;

/// The first water at height `h` (game z) that some part of lies inside the main
/// camera's view (`clip_from_world`, Bevy space): the part of every loaded cell and every
/// distant-water cell where the water stands above the land ([`DistantWater::wet_area`],
/// so planes under the ground never count), and placed water
/// objects whose bounds reach that height. Water at another height reflects only the sky
/// and does not need the camera. What hides it from view (a hill, a wall) is not taken
/// into account.
fn water_on_screen(
    world: &WorldData,
    streamer: &Streamer,
    dw: &mut DistantWater,
    placed: &PlacedWater,
    clip_from_world: Mat4,
    h: f32,
    cam: Vec3,
) -> Option<SeenWater> {
    // Nearest first, so the water named is the same every run (the maps' order is not).
    let g = bevy_to_game(cam);
    let cells = nearest_first(streamer.loaded.keys().copied().chain(dw.plane_cells()), cell_of(g.x, g.y));
    for c in cells {
        let Some((height, _)) = dw.surface(world, c) else { continue };
        if (height - h).abs() > SAME_HEIGHT {
            continue;
        }
        let Some(wet) = dw.wet_area(world, c) else { continue };
        if polygon_in_view(clip_from_world, &wet_quad(wet, height)) {
            return Some(SeenWater::Cell(c));
        }
    }
    placed_water_on_screen(placed.iter().map(|(t, aabb, r, v)| (t.affine(), aabb, r.0, v.get())), clip_from_world, h, cam)
}

/// The nearest placed water object (to `cam`, Bevy space; the lower FormID on a tie) that
/// is drawn, reaches height `h` and lies partly inside the view: each as its transform,
/// bounds, reference and whether it is visible (an object hidden or on a disabled
/// reference never counts, #1059).
fn placed_water_on_screen<'a>(
    placed: impl Iterator<Item = (bevy::math::Affine3A, &'a Aabb, u32, bool)>,
    clip_from_world: Mat4,
    h: f32,
    cam: Vec3,
) -> Option<SeenWater> {
    let mut best: Option<(f32, u32)> = None;
    for (m, aabb, r, visible) in placed {
        if !visible {
            continue;
        }
        let (c, e) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        let corners: [Vec3; 8] = std::array::from_fn(|i| {
            let s = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            );
            m.transform_point3(c + e * s)
        });
        // Bevy y is game z.
        let (lo, hi) = corners.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        if h < lo - SAME_HEIGHT || h > hi + SAME_HEIGHT {
            continue;
        }
        if box_in_view(clip_from_world, &corners) {
            // The nearest one seen, the lower FormID on a tie.
            let d = m.transform_point3(c).distance_squared(cam);
            if best.is_none_or(|(bd, bf)| (d, r) < (bd, bf)) {
                best = Some((d, r));
            }
        }
    }
    best.map(|(_, f)| SeenWater::Placed(f))
}

/// `cells` without repeats, nearest `centre` first (by grid steps both ways, then the
/// cell's own coordinates), so the order does not depend on the maps they came from.
fn nearest_first(cells: impl Iterator<Item = (i32, i32)>, centre: (i32, i32)) -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = cells.collect();
    v.sort_unstable_by_key(|&(x, y)| ((x - centre.0).abs().max((y - centre.1).abs()), (x - centre.0).abs() + (y - centre.1).abs(), x, y));
    v.dedup();
    v
}

/// The wet part of a cell (`[x0, y0, x1, y1]`, game units) at game height `height`, its
/// corners in Bevy space.
fn wet_quad(wet: [f32; 4], height: f32) -> [Vec3; 4] {
    let [x0, y0, x1, y1] = wet;
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| game_point(Vec3::new(x, y, height)))
}

/// A game-space point in Bevy space (the inverse of [`bevy_to_game`]).
fn game_point(g: Vec3) -> Vec3 {
    Vec3::new(g.x, g.z, -g.y)
}

/// The sides of Bevy's view volume in clip space, each as `n · p >= 0`: left, right,
/// bottom, top, near (`z <= w`; reverse z puts the near plane at 1) and far (`z >= 0`;
/// the far plane is at infinity).
const VIEW_SIDES: [Vec4; 6] = [
    Vec4::new(1.0, 0.0, 0.0, 1.0),
    Vec4::new(-1.0, 0.0, 0.0, 1.0),
    Vec4::new(0.0, 1.0, 0.0, 1.0),
    Vec4::new(0.0, -1.0, 0.0, 1.0),
    Vec4::new(0.0, 0.0, -1.0, 1.0),
    Vec4::new(0.0, 0.0, 1.0, 0.0),
];

/// Whether any part of the convex polygon `points` (world space) lies inside the view:
/// the polygon clipped against each side of the view volume in clip space
/// (Sutherland-Hodgman) keeps something.
fn polygon_in_view(clip_from_world: Mat4, points: &[Vec3]) -> bool {
    let mut poly: Vec<Vec4> = points.iter().map(|p| clip_from_world * p.extend(1.0)).collect();
    for side in VIEW_SIDES {
        if poly.is_empty() {
            return false;
        }
        let mut out = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (da, db) = (side.dot(a), side.dot(b));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                out.push(a + (b - a) * (da / (da - db)));
            }
        }
        poly = out;
    }
    !poly.is_empty()
}

/// Whether any part of the box with these 8 corners (bit 0: its first axis, bit 1: its
/// second, bit 2: its third) lies inside the view: one of its faces does, or the camera
/// is inside it.
fn box_in_view(clip_from_world: Mat4, corners: &[Vec3; 8]) -> bool {
    const FACES: [[usize; 4]; 6] = [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]];
    if FACES.iter().any(|f| polygon_in_view(clip_from_world, &f.map(|i| corners[i]))) {
        return true;
    }
    // The camera inside the box (swimming in it): every face may lie outside the view.
    let eye = clip_from_world.inverse().project_point3(Vec3::new(0.0, 0.0, 1.0));
    let (o, ex, ey, ez) = (corners[0], corners[1] - corners[0], corners[2] - corners[0], corners[4] - corners[0]);
    let d = eye - o;
    [ex, ey, ez].iter().all(|axis| {
        let t = d.dot(*axis);
        t >= 0.0 && t <= axis.length_squared()
    })
}

/// The water surface (Bevy y) to reflect in: the nearest cell around the camera with
/// visible water below the camera.
fn reflection_height(world: &WorldData, dw: &mut DistantWater, cam_game: Vec3) -> Option<f32> {
    let c = cell_of(cam_game.x, cam_game.y);
    (0..=SEARCH).find_map(|r| {
        (-r..=r)
            .flat_map(|dx| (-r..=r).map(move |dy| (dx, dy)))
            .filter(|&(dx, dy)| dx.abs().max(dy.abs()) == r)
            .filter_map(|(dx, dy)| dw.surface(world, (c.0 + dx, c.1 + dy)))
            .map(|(h, _)| h)
            .filter(|&h| h < cam_game.z)
            .min_by(|a, b| (cam_game.z - a).total_cmp(&(cam_game.z - b)))
    })
}

/// Test aid `NVF_PLAY_TEST_NO_REFLECTION=1`: the camera stays off (water reflects the sky),
/// for measuring what it costs where water is seen (docs/design/frame-rate-audit.md §1).
fn no_reflection() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("NVF_PLAY_TEST_NO_REFLECTION").is_ok_and(|v| v == "1"))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_reflection(
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    mut dw: ResMut<DistantWater>,
    main: Query<(&Camera, &GlobalTransform, &Projection, &DistanceFog), crate::fly::WorldCamera>,
    mut refl: Query<
        (&mut Camera, &mut Transform, &mut GlobalTransform, &mut Projection, &mut DistanceFog),
        With<ReflectionCamera>,
    >,
    cache: Res<WaterCache>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    graphics: Option<Res<crate::graphics::Graphics>>,
    placed: PlacedWater,
    time: Res<Time<Real>>,
    hidden: Option<Res<crate::console_toggles::Hidden>>,
    mut probe: ResMut<WaterProbe>,
    mut pixels: ResMut<WaterPixels>,
    mut gate: ResMut<ReflectionGate>,
) {
    let Ok((mut camera, mut t, mut gt, mut proj, mut fog)) = refl.single_mut() else { return };
    let main = main.single().ok().filter(|(camera, ..)| camera.is_active);
    let mirror = graphics.as_ref().is_none_or(|g| g.water_mirror);
    let water_drawn = !hidden.is_some_and(|h| h.water);
    let near_water = match (streamer.interior, main) {
        (None, Some((_, main_gt, Projection::Perspective(_), _))) if mirror && water_drawn && !no_reflection() => {
            reflection_height(&world, &mut dw, bevy_to_game(main_gt.translation()))
        }
        _ => None,
    };
    let seen = near_water.zip(main).and_then(|(h, (_, main_gt, main_proj, _))| {
        let clip_from_world = main_proj.get_clip_from_view() * main_gt.to_matrix().inverse();
        water_on_screen(&world, &streamer, &mut dw, &placed, clip_from_world, h, main_gt.translation())
    });
    let now = time.elapsed_secs();
    let candidate = near_water.map(|h| (world.world, h));
    let target = candidate.filter(|_| seen.is_some());
    let buffer = cache.seen_counter().map(Handle::id);
    if probe.target != target || probe.buffer != buffer {
        probe.target = target;
        probe.buffer = buffer;
        probe.generation = probe.generation.wrapping_add(1);
        pixels.started = now;
        pixels.last = None;
        pixels.last_drawn = None;
        pixels.empty_since = None;
    }
    probe.frame = probe.frame.wrapping_add(1);
    probe.time = now;
    cache.set_seen_plane(&mut materials, probe.seen_plane());

    let same_plane = gate.plane == candidate;
    if !same_plane {
        gate.last_in_view = None;
    }
    gate.plane = candidate;
    if seen.is_some() {
        gate.last_in_view = Some(now);
    }
    let gpu = pixels.state(now);
    let keep_active = camera.is_active
        && same_plane
        && gate.last_in_view.is_some_and(|last| now - last <= KEEP_UNSEEN)
        && gpu != GpuWater::Hidden;
    let start = buffer.is_some() && seen.is_some() && matches!(gpu, GpuWater::Drawn | GpuWater::Unavailable);
    let height = near_water.filter(|_| keep_active || start);
    // z: screen-space reflections stand in where this plane is not (graphics.rs).
    let ssr = if graphics.is_some_and(|g| g.water_reflections) { 1.0 } else { 0.0 };
    let plane = match height {
        Some(h) => Vec4::new(h, 1.0, ssr, 0.0),
        None => Vec4::new(0.0, 0.0, ssr, 0.0),
    };
    cache.set_reflection_plane(&mut materials, plane);
    if camera.is_active != height.is_some() {
        camera.is_active = height.is_some();
        match (height, seen) {
            (Some(_), Some(SeenWater::Cell(c))) => info!("water reflection: on (the water of cell {},{} on screen)", c.0, c.1),
            (Some(_), Some(SeenWater::Placed(r))) => info!("water reflection: on (placed water {r:08X} on screen)"),
            (Some(_), None) => {}
            (None, _) if !mirror => info!("water reflection: off (mirrored water switched off)"),
            (None, _) if !water_drawn => info!("water reflection: off (water hidden)"),
            (None, _) if gpu == GpuWater::Hidden => info!(
                "water reflection: off (no water pixels sampled for {GPU_HIDDEN_HOLD} s)"
            ),
            (None, _) if near_water.is_some() && !same_plane => info!("water reflection: off (plane changed)"),
            (None, _) if near_water.is_some() => info!("water reflection: off (out of view for {KEEP_UNSEEN} s)"),
            (None, _) => info!("water reflection: off (no eligible plane)"),
        }
    }
    let (Some(h), Some((_, main_gt, Projection::Perspective(p), main_fog))) = (height, main) else { return };
    let cam = main_gt.translation();

    // Mirror the camera in the plane y = h.
    let mirror = Mat4::from_translation(Vec3::Y * h)
        * Mat4::from_mat3a(reflection_matrix(Vec3::Y))
        * Mat4::from_translation(Vec3::NEG_Y * h);
    *t = Transform::from_matrix(mirror * main_gt.to_matrix());
    *gt = GlobalTransform::from(*t);
    // Keep only what lies above the plane: in the main camera's view space the mirrored
    // scene must lie below it (see Bevy's mirror example).
    let normal = (main_gt.compute_transform().rotation.inverse() * Vec3::NEG_Y).normalize();
    *proj = Projection::Perspective(PerspectiveProjection {
        near_clip_plane: normal.extend(h - CLIP_MARGIN - cam.y),
        ..p.clone()
    });
    *fog = main_fog.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authored_test_records::{record, sub, tes4, top, z};
    use crate::fly::FlyCam;
    use crate::lod::LodState;
    use crate::world::{game_to_bevy, ReadAhead, StreamStats, WorldRoot};
    use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
    use fnv_esm::{LoadOrder, Plugin as EsmPlugin, Schema};
    use fnv_world::index::{Cell, Worldspace};
    use nvf_assets::{GameData, ModelCache, NifMaterial, TextureCache};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::LazyLock;

    /// The clip-from-world matrix of a camera at `eye` (Bevy space) turned `pitch` degrees
    /// up from looking along -z, with the game's 75° view and Bevy's reverse-z projection.
    fn view(eye: Vec3, pitch: f32) -> Mat4 {
        let proj = PerspectiveProjection { fov: 75f32.to_radians(), aspect_ratio: 16.0 / 9.0, near: 5.0, ..default() };
        let world_from_view = Transform::from_translation(eye).with_rotation(Quat::from_rotation_x(pitch.to_radians())).to_matrix();
        Projection::Perspective(proj).get_clip_from_view() * world_from_view.inverse()
    }

    /// A horizontal square of half-width `r` at Bevy height `y`, centred under `(x, z)`.
    fn square(x: f32, z: f32, y: f32, r: f32) -> [Vec3; 4] {
        [Vec3::new(x - r, y, z - r), Vec3::new(x + r, y, z - r), Vec3::new(x + r, y, z + r), Vec3::new(x - r, y, z + r)]
    }

    #[test]
    fn water_ahead_and_below_is_seen() {
        let v = view(Vec3::new(0.0, 100.0, 0.0), -10.0);
        assert!(polygon_in_view(v, &square(0.0, -3000.0, 0.0, 500.0)));
    }

    #[test]
    fn water_behind_is_not_seen() {
        let v = view(Vec3::new(0.0, 100.0, 0.0), -10.0);
        assert!(!polygon_in_view(v, &square(0.0, 3000.0, 0.0, 500.0)));
    }

    #[test]
    fn water_off_to_the_side_is_not_seen() {
        let v = view(Vec3::new(0.0, 100.0, 0.0), 0.0);
        assert!(!polygon_in_view(v, &square(6000.0, 0.0, 0.0, 500.0)));
    }

    #[test]
    fn a_huge_lake_under_a_camera_looking_at_the_sky_is_not_seen() {
        // Bevy's bounding-box test passes such a plane; the clip test does not.
        let v = view(Vec3::new(0.0, 100.0, 0.0), 70.0);
        assert!(!polygon_in_view(v, &square(0.0, 0.0, 0.0, 20_000.0)));
        // Looking level, the far part of the same lake is on screen below the horizon.
        assert!(polygon_in_view(view(Vec3::new(0.0, 100.0, 0.0), 0.0), &square(0.0, 0.0, 0.0, 20_000.0)));
    }

    #[test]
    fn water_crossing_the_near_plane_is_seen() {
        // Half the square lies behind the camera (negative w), the rest in view.
        let v = view(Vec3::new(0.0, 10.0, 0.0), -60.0);
        assert!(polygon_in_view(v, &square(0.0, 0.0, 0.0, 50.0)));
    }

    #[test]
    fn water_above_the_camera_is_seen_only_looking_up() {
        let s = square(0.0, -500.0, 300.0, 400.0);
        assert!(polygon_in_view(view(Vec3::ZERO, 45.0), &s));
        assert!(!polygon_in_view(view(Vec3::ZERO, -60.0), &s));
    }

    #[test]
    fn a_box_around_the_camera_counts_as_seen() {
        let corners: [Vec3; 8] = std::array::from_fn(|i| {
            Vec3::new(if i & 1 == 0 { -1000.0 } else { 1000.0 }, if i & 2 == 0 { -1000.0 } else { 1000.0 }, if i & 4 == 0 { -1000.0 } else { 1000.0 })
        });
        assert!(box_in_view(view(Vec3::ZERO, 0.0), &corners));
        let far: [Vec3; 8] = corners.map(|c| c * 0.1 + Vec3::new(0.0, 0.0, 9000.0));
        assert!(!box_in_view(view(Vec3::ZERO, 0.0), &far));
    }

    #[test]
    fn cells_come_nearest_first_whatever_their_order() {
        let a = nearest_first([(5, 5), (0, 1), (2, 0), (0, 0), (1, 1), (0, 1)].into_iter(), (0, 0));
        let b = nearest_first([(1, 1), (0, 1), (5, 5), (2, 0), (0, 0)].into_iter(), (0, 0));
        assert_eq!(a, vec![(0, 0), (0, 1), (1, 1), (2, 0), (5, 5)]);
        assert_eq!(a, b);
    }

    /// #1059: placed water that is not drawn (a disabled reference, a hidden model) never
    /// switches the camera on; the same object drawn does. Of two drawn, the nearer one is
    /// named (eb618878's choice).
    #[test]
    fn hidden_placed_water_is_not_seen() {
        let eye = Vec3::new(0.0, 100.0, 0.0);
        let v = view(eye, -10.0);
        let aabb = Aabb::from_min_max(Vec3::new(-500.0, -10.0, -500.0), Vec3::new(500.0, 0.0, 500.0));
        let near = bevy::math::Affine3A::from_translation(Vec3::new(0.0, 0.0, -3000.0));
        let far = bevy::math::Affine3A::from_translation(Vec3::new(0.0, 0.0, -6000.0));
        let one = |visible| placed_water_on_screen(std::iter::once((near, &aabb, 0x1234, visible)), v, 0.0, eye);
        assert_eq!(one(true), Some(SeenWater::Placed(0x1234)));
        assert_eq!(one(false), None);
        // At another height: not the reflected plane's water.
        assert_eq!(placed_water_on_screen(std::iter::once((near, &aabb, 0x1234, true)), v, 200.0, eye), None);
        // The nearer of two drawn; a hidden nearer one is passed over for the farther.
        let two = |near_visible| placed_water_on_screen([(far, &aabb, 0x1, true), (near, &aabb, 0x2, near_visible)].into_iter(), v, 0.0, eye);
        assert_eq!(two(true), Some(SeenWater::Placed(0x2)));
        assert_eq!(two(false), Some(SeenWater::Placed(0x1)));
    }

    const WATER: u32 = 0x800;

    struct EmptyData(PathBuf);
    impl Drop for EmptyData {
        fn drop(&mut self) {
            std::fs::remove_dir(&self.0).unwrap();
        }
    }

    // WorldData intentionally borrows process-lifetime game records. Share the
    // synthetic records across tests without leaking a separate fixture per app.
    static PLUGINS: LazyLock<[EsmPlugin; 1]> = LazyLock::new(|| {
        // WaterVisual's documented record fields, as decoded by schemas/fnv/sky.json.
        // No image files or installed records are needed by this authored water type.
        let mut visual = vec![0; 196];
        visual[44..48].copy_from_slice(&[20, 170, 210, 0]);
        visual[144..148].copy_from_slice(&7_f32.to_le_bytes());
        visual[148..152].copy_from_slice(&321_f32.to_le_bytes());
        let bytes = [
            tes4(),
            top(
                b"WATR",
                &record(
                    b"WATR",
                    0,
                    WATER,
                    &[
                        sub(b"EDID", z("AuthoredReflectionWater")),
                        sub(b"DNAM", visual),
                    ],
                ),
            ),
        ]
        .concat();
        [EsmPlugin::parse(&bytes, "AuthoredReflection.esm".into()).unwrap()]
    });
    static LOAD_ORDER: LazyLock<LoadOrder<'static>> = LazyLock::new(|| LoadOrder::new(&*PLUGINS).unwrap());
    static SCHEMA: LazyLock<Schema> = LazyLock::new(|| Schema::load_dir(schemas().join("fnv")).unwrap());
    static IMPOSTERS: LazyLock<fnv_world::imposter::ImposterRegions> =
        LazyLock::new(|| fnv_world::imposter::ImposterRegions::build(&LOAD_ORDER, &SCHEMA));
    static INDEX: LazyLock<fnv_world::WorldIndex> = LazyLock::new(|| {
        let mut index = fnv_world::WorldIndex::default();
        let mut ws = Worldspace {
            form_id: 1,
            water: Some(WATER),
            ..Default::default()
        };
        for (n, grid) in [(-1, 0), (0, 0), (1, 0), (4, 0)].into_iter().enumerate() {
            let id = 0x900 + n as u32;
            ws.grid.insert(grid, id);
            index.cells.insert(
                id,
                Cell {
                    form_id: id,
                    world: Some(1),
                    grid: Some(grid),
                    data_flags: 2,
                    water_height: Some(100.0),
                    water_type: Some(WATER),
                    ..Default::default()
                },
            );
        }
        index.worldspaces.insert(1, ws);
        index
    });

    fn schemas() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
    }

    fn reflection_fixture() -> (App, Entity, EmptyData) {
        let decoded = fnv_world::sky::water(&LOAD_ORDER, &SCHEMA, WATER).unwrap();
        assert_eq!(decoded.deep, [20, 170, 210]);
        assert_eq!(decoded.under_fog, [7.0, 321.0, 0.0]);
        let world = WorldData {
            lo: &LOAD_ORDER,
            schema: &SCHEMA,
            index: &INDEX,
            world: 1,
            persistent_by_grid: HashMap::new(),
            imposters: &IMPOSTERS,
            weather_regions: &[],
            dropped_plugins: Vec::new(),
        };
        let mut lod = LodState::new(&world, crate::lod::LodSettings::default(), true, None);
        // The chunks the old LOD square of radius 0 covered round (0, 0): cells -4..=3 by
        // 0..=3, so (-1, 0), (0, 0) and (1, 0), not (4, 0).
        lod.choose_for_test(vec![(4, -4, 0), (4, 0, 0)]);
        let streamer = Streamer {
            radius: 1,
            loaded: HashMap::new(),
            fades: HashMap::new(),
            fade_secs: 0.0,
            settled: true,
            arrival: None,
            per_frame: 1,
            spawning: HashMap::new(),
            dying: Vec::new(),
            ahead: ReadAhead::default(),
            hide: Vec::new(),
            interior: None,
            light_scale: 1.0,
            managed: HashSet::new(),
            manage_actors: false,
            stats: StreamStats::default(),
            buffer: Default::default(),
            centre: None,
            waiting: Vec::new(),
            model_overrides: HashMap::new(),
            coop_areas: Vec::new(),
            travels: 0,
        };
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = EmptyData(std::env::temp_dir().join(format!(
            "nvf-water-reflection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::create_dir(&dir.0).unwrap();
        let data = GameData::open_for_plugins(&dir.0, &[] as &[&str], &schemas().join("nif")).unwrap();
        let mut a = App::new();
        a.add_plugins(TransformPlugin)
            .insert_resource(world)
            .insert_resource(lod)
            .insert_resource(streamer)
            .insert_resource(data)
            .insert_resource(crate::stream_pace::StreamPace::new(crate::stream_pace::Mode::Off))
            .init_resource::<DistantWater>()
            .init_resource::<WaterCache>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<NifMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<WaterMaterial>>()
            .init_resource::<Assets<ShaderBuffer>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<ModelCache>()
            .init_resource::<TextureCache>();
        a.world_mut().spawn((WorldRoot, game_to_bevy()));
        let camera = a
            .world_mut()
            .spawn((
                Camera3d::default(),
                FlyCam {
                    yaw: 0.0,
                    pitch: 0.0,
                    speed: 0.0,
                },
                Transform::from_xyz(1024.0, 200.0, -1024.0),
                DistanceFog::default(),
            ))
            .id();
        (a, camera, dir)
    }

    fn scheduled_reflection() -> (App, Entity, EmptyData) {
        let (mut app, camera, data) = reflection_fixture();
        app.add_message::<WindowResized>()
            .init_resource::<Time<Real>>()
            .init_resource::<crate::console_toggles::Hidden>()
            .insert_resource(crate::graphics::Graphics::preset(crate::graphics::Preset::Enhanced))
            .add_plugins(ReflectionPlugin)
            .add_systems(Update, crate::water::distant_water);
        // distant_water budgets uncached surface reads per frame (#2956), so settle the
        // initial r = 3 square around (0, 0) at t = 0 before timed reflection checks.
        for _ in 0..6 {
            app.update();
            if app.world().resource::<DistantWater>().plane_cells().count() == 3 {
                break;
            }
        }
        assert_eq!(app.world().resource::<DistantWater>().plane_cells().count(), 3);
        (app, camera, data)
    }

    fn tick(app: &mut App, seconds: f32) {
        app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f32(seconds));
        app.update();
    }

    fn probe_sample(app: &App, count: u32) -> WaterSample {
        let probe = app.world().resource::<WaterProbe>();
        assert_eq!(probe.target, Some((1, 100.0)), "the real water producer must supply a visible plane");
        assert!(probe.buffer.is_some());
        let materials = app.world().resource::<Assets<WaterMaterial>>();
        let (_, material) = materials.iter().next().unwrap();
        assert_eq!(Some(material.seen.id()), probe.buffer);
        assert_eq!(
            material.params.seen_plane,
            Vec4::new(100.0, 1.0, (probe.generation & 0xffff) as f32, (probe.generation >> 16) as f32)
        );
        probe.sample(count)
    }

    fn deliver(app: &mut App, sample: WaterSample) {
        let (_, entity) = app.world().resource::<WaterPixels>().requested.unwrap();
        app.world_mut().trigger(ReadbackComplete { entity, data: sample.bytes().to_vec() });
    }

    fn assert_reflection(app: &mut App, active: bool) {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&Camera, &GlobalTransform), With<ReflectionCamera>>();
        let (camera, transform) = query.single(world).unwrap();
        assert_eq!(camera.is_active, active);
        assert_eq!(camera.order, -1);
        if active {
            assert!(transform.translation().y.abs() < 0.001, "mirror camera must use the 100-unit water plane");
        }
        let materials = world.resource::<Assets<WaterMaterial>>();
        assert_eq!(materials.len(), 1);
        let (_, material) = materials.iter().next().unwrap();
        let expected = if active { Vec4::new(100.0, 1.0, 1.0, 0.0) } else { Vec4::new(0.0, 0.0, 1.0, 0.0) };
        assert_eq!(material.params.reflection_plane, expected);
    }

    #[test]
    fn scheduled_reflection_does_not_start_for_occluded_water() {
        let (mut app, _, _data) = scheduled_reflection();
        tick(&mut app, 0.25);
        // The actual plane and material exist, but the GPU never sees through the
        // occluder. A working stream of zeros must not trigger the missing-stream fallback.
        for _ in 0..=SAMPLE_PHASES {
            let empty = probe_sample(&app, 0);
            deliver(&mut app, empty);
            tick(&mut app, 0.25);
            assert_reflection(&mut app, false);
        }
    }

    #[test]
    fn scheduled_reflection_uses_batched_feedback_and_sustained_empty_frames() {
        let (mut app, _, _data) = scheduled_reflection();
        tick(&mut app, 0.25);
        assert_reflection(&mut app, false);
        let drawn = probe_sample(&app, 25);
        tick(&mut app, 0.25);
        let empty = probe_sample(&app, 0);
        // Extraction can deliver several completions ahead of a single camera update.
        deliver(&mut app, drawn);
        deliver(&mut app, empty);
        tick(&mut app, 0.25);
        assert_reflection(&mut app, true);

        for _ in 0..15 {
            let empty = probe_sample(&app, 0);
            deliver(&mut app, empty);
            tick(&mut app, 0.25);
            assert_reflection(&mut app, true);
        }
        let empty = probe_sample(&app, 0);
        deliver(&mut app, empty);
        tick(&mut app, 0.25);
        assert_reflection(&mut app, false);

        // A late older result does not reactivate the camera; a fresh one does.
        deliver(&mut app, WaterSample { count: 1, frame: empty.frame.wrapping_sub(1), time: empty.time - 0.25, ..empty });
        tick(&mut app, 0.25);
        assert_reflection(&mut app, false);
        let drawn = probe_sample(&app, 1);
        deliver(&mut app, drawn);
        tick(&mut app, 0.25);
        assert_reflection(&mut app, true);
    }

    #[test]
    fn scheduled_reflection_requires_current_feedback_and_honors_cpu_shutdown() {
        let (mut app, main, _data) = scheduled_reflection();
        tick(&mut app, 0.25);
        let stale = probe_sample(&app, 1);
        // No feedback at all: keep the existing CPU-only fallback.
        tick(&mut app, READBACK_WAIT);
        assert_reflection(&mut app, true);

        let look_away = Quat::from_rotation_x(80f32.to_radians());
        app.world_mut().get_mut::<Transform>(main).unwrap().rotation = look_away;
        tick(&mut app, 0.25);
        assert_reflection(&mut app, true);
        tick(&mut app, KEEP_UNSEEN);
        assert_reflection(&mut app, false);

        app.world_mut().get_mut::<Transform>(main).unwrap().rotation = Quat::IDENTITY;
        tick(&mut app, 0.25);
        let current = probe_sample(&app, 1);
        assert_ne!(current.generation, stale.generation);
        deliver(&mut app, WaterSample { generation: stale.generation, ..current });
        deliver(&mut app, WaterSample { time: stale.time, ..current });
        tick(&mut app, 0.25);
        assert_reflection(&mut app, false);
        deliver(&mut app, current);
        tick(&mut app, 0.25);
        assert_reflection(&mut app, true);

        // Wall time with missing completions is not a run of empty drawn frames.
        tick(&mut app, GPU_HIDDEN_HOLD + 1.0);
        let resumed = probe_sample(&app, 0);
        deliver(&mut app, resumed);
        tick(&mut app, 0.25);
        assert_reflection(&mut app, true);

        app.world_mut().resource_mut::<crate::console_toggles::Hidden>().water = true;
        tick(&mut app, 0.25);
        assert_reflection(&mut app, false);
        app.world_mut().resource_mut::<crate::console_toggles::Hidden>().water = false;
        tick(&mut app, 0.25);
        let drawn = probe_sample(&app, 1);
        deliver(&mut app, drawn);
        tick(&mut app, 0.25);
        assert_reflection(&mut app, true);
        app.world_mut().resource_mut::<crate::graphics::Graphics>().water_mirror = false;
        tick(&mut app, 0.25);
        assert_reflection(&mut app, false);
        assert!(app.world().resource::<WaterProbe>().target.is_none());
    }

    #[derive(Resource)]
    struct CounterFixture(Handle<ShaderBuffer>);

    fn request_fixture(mut commands: Commands, counter: Res<CounterFixture>, mut px: ResMut<WaterPixels>) {
        request_android_water_pixels(&mut commands, &counter.0, &mut px);
    }

    fn readback_app() -> App {
        let mut app = App::new();
        // Real one-shot removal runs in First; no RenderApp or GPU device is made.
        app.add_plugins((MinimalPlugins, bevy::render::gpu_readback::GpuReadbackPlugin::default()))
            .init_resource::<Assets<ShaderBuffer>>()
            .init_resource::<WaterPixels>()
            .insert_resource(WaterProbe { target: Some((0, 0.0)), buffer: None, generation: 1, frame: 1, time: 0.0 })
            .add_systems(Update, request_fixture);
        let counter = app.world_mut().resource_mut::<Assets<ShaderBuffer>>()
            .add(ShaderBuffer::new(vec![0_u32; 4], default()));
        app.insert_resource(CounterFixture(counter));
        app
    }

    fn request_entities(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &Name)>();
        query.iter(world).filter(|(_, name)| name.as_str() == "Water pixel readback").map(|(entity, _)| entity).collect()
    }

    fn complete_readback(app: &mut App, entity: Entity, data: Vec<u8>) {
        app.world_mut().trigger(ReadbackComplete { entity, data });
        app.world_mut().flush();
    }

    #[test]
    fn android_water_readback_stall_retains_one_request_and_existing_fallback() {
        let mut app = readback_app();
        app.update();
        let first = app.world().resource::<WaterPixels>().pending.unwrap().0;
        assert_eq!(request_entities(&mut app), vec![first]);
        assert!(app.world().get::<ReadbackOnce>(first).is_some());
        for _ in 1..=180 {
            app.update();
            assert_eq!(app.world().resource::<WaterPixels>().pending.unwrap().0, first);
            assert_eq!(request_entities(&mut app), vec![first], "stalled callback cannot allocate more request entities");
            assert!(app.world().get::<Readback>(first).is_none(), "real plugin removed the one-shot request after its single extraction frame");
        }
        assert_eq!(app.world().resource::<WaterPixels>().state(READBACK_WAIT + 1.0), GpuWater::Unavailable, "existing initial timeout fallback survives without a callback");
        assert!(app.world().resource::<WaterPixels>().last.is_none());
    }

    #[test]
    fn android_water_readback_completion_retires_once_and_preserves_counter_semantics() {
        let mut app = readback_app();
        for (i, count) in [10_u32, 0, 25, 0].into_iter().enumerate() {
            let now = app.world().resource::<Time<Real>>().elapsed_secs();
            let sample = WaterSample { count, generation: 1, frame: i as u32 + 1, time: now };
            app.update();
            let entity = app.world().resource::<WaterPixels>().pending.unwrap().0;
            assert_eq!(request_entities(&mut app), vec![entity]);
            complete_readback(&mut app, entity, sample.bytes().to_vec());
            assert!(app.world().resource::<WaterPixels>().pending.is_none());
            assert!(request_entities(&mut app).is_empty(), "completion retires the actual request entity");
            let px = app.world().resource::<WaterPixels>();
            assert_eq!(px.last.map(|s| s.count), Some(count));
            assert_eq!(px.state(now), GpuWater::Drawn);
        }
        app.update();
        assert_eq!(request_entities(&mut app).len(), 1, "one successor is admitted after the previous completion");
    }

    #[test]
    fn android_water_readback_late_old_counter_and_short_result_cannot_pollute_new_counter() {
        let mut app = readback_app();
        app.update();
        let old = app.world().resource::<WaterPixels>().pending.unwrap();
        let replacement = app.world_mut().resource_mut::<Assets<ShaderBuffer>>()
            .add(ShaderBuffer::new(vec![0_u32; 4], default()));
        app.insert_resource(CounterFixture(replacement.clone()));
        app.update();
        assert_eq!(request_entities(&mut app), vec![old.0], "counter replacement must still wait for the existing request");
        assert_eq!(app.world().resource::<WaterPixels>().counter, Some(replacement.id()));
        let now = app.world().resource::<Time<Real>>().elapsed_secs();
        let old_sample = WaterSample { count: 999, generation: 1, frame: 1, time: now };
        complete_readback(&mut app, old.0, old_sample.bytes().to_vec());
        let px = app.world().resource::<WaterPixels>();
        assert!(px.pending.is_none());
        assert!(px.last.is_none(), "late old-counter bytes cannot become the new counter baseline");
        assert!(px.last_drawn.is_none());
        app.update();
        let fresh = app.world().resource::<WaterPixels>().pending.unwrap();
        assert_eq!(fresh.1, replacement.id());
        complete_readback(&mut app, fresh.0, vec![1, 2, 3]);
        assert!(app.world().resource::<WaterPixels>().last.is_none(), "short payload is not a counter value");
        app.update();
        let fresh = app.world().resource::<WaterPixels>().pending.unwrap().0;
        let now = app.world().resource::<Time<Real>>().elapsed_secs();
        let valid = WaterSample { count: 7, generation: 1, frame: 2, time: now };
        complete_readback(&mut app, fresh, valid.bytes().to_vec());
        assert_eq!(app.world().resource::<WaterPixels>().last.map(|s| s.count), Some(7));
        assert_eq!(app.world().resource::<WaterPixels>().state(now), GpuWater::Drawn);
        assert!(request_entities(&mut app).is_empty());
    }

    #[test]
    fn game_points_map_to_bevy_and_back() {
        let g = Vec3::new(10.0, 20.0, 30.0);
        assert_eq!(bevy_to_game(game_point(g)), g);
    }
}
