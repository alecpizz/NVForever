//! Seamless doors, step 4 (docs/design/seamless-doors.md §5; `nvf_play::seamless`): a
//! load door whose other side is loaded (kept, step 3) opens onto it. The doorway shows
//! that place, drawn by a camera of its own, and walking into the doorway is the travel,
//! with no fade and no loading screen. NVForever's own; the original has no such thing.
//!
//! - **Which doors**: the doors near the player (`DoorsAhead`) whose destination is the
//!   place parked (`world::Parked`) are told to nvf-play every frame (`PortalDoors`);
//!   activating one sends `OpenPortal` instead of a travel.
//! - **The map between the two sides**: each door's outward direction is taken from where
//!   the other door's travel arrives (in front of it) [design: the doors' own rotations
//!   do not say which side is out]. A point in front of this door maps to the same place
//!   in front of the other door's inside, [`SHIFT`] further in, so that where the player is
//!   stopped by the closed door's collision maps to a free spot in the room: the travel
//!   puts the player exactly where the portal camera was, and the view does not jump.
//!   Heights go floor to floor (the arrival markers' heights).
//! - **The view**: the parked place's entities are put on [`PORTAL_LAYER`] (those on the
//!   world's layer 0; others, shadow-only proxies, keep theirs) and shown; a camera on that
//!   layer, moved with the main view through the map, draws them to a texture, clipped at
//!   the far door's plane (an oblique near plane, as the water reflection). The doorway's
//!   quad samples it at its own screen position ([`PortalMaterial`]). That camera has the
//!   place's own light: an interior's (`SkyState::interior_look`), or the outdoors' as last
//!   seen outdoors. Both doors' models are hidden while it is open.
//! - **Crossing**: the player within [`CROSS_DISTANCE`] of the door's plane, inside its
//!   width and looking into it, is sent through (`nvf_play::Travel`, the place left kept);
//!   the door it came out of opens behind it the same way, until the player walks off.
//! - **Not built**: the sky, distant land and other actors than those drawn in the
//!   parked cells are not in the view through a door; a door that has not been through
//!   once (its other side not parked yet) still fades.

use crate::sky::{PlaceLook, SkyState};
use crate::world::{bevy_to_game, game_to_bevy, Streamer, WorldData};
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{AmbientLight, GlobalAmbientLight};
use bevy::math::Affine3A;
use bevy::pbr::{DistanceFog, Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::shader::ShaderRef;
use bevy::window::PrimaryWindow;
use nvf_play::seamless::{DoorsAhead, LoadDoor, LoadDoors, OpenPortal, PortalDoors, Seamless, TravelKeeps};
use nvf_play::RefVisual;
use std::collections::HashMap;

/// The render layer the parked place is drawn on while a door shows it.
pub const PORTAL_LAYER: usize = 20;

/// The player's feet this near the door's plane (game units), inside its width and looking
/// into it, go through [engineering choice: the player's capsule stops at about its radius
/// from the closed door's collision].
pub const CROSS_DISTANCE: f32 = 48.0;

/// How much further in than the other door the map puts a point (game units)
/// [engineering choice: [`CROSS_DISTANCE`] plus room for the capsule, so the travel lands
/// clear of the far door].
pub const SHIFT: f32 = CROSS_DISTANCE + 40.0;

/// An open door closes once the player is this far from it (game units) [engineering
/// choice].
pub const CLOSE_DISTANCE: f32 = 700.0;

/// After going through, the player cannot go back through for this long (real seconds)
/// [engineering choice: the door behind opens at once].
pub const CROSS_COOLDOWN: f64 = 0.75;

/// A doorway's size when its door's drawn bounds are not known (game units) [design: a
/// house door].
const DEFAULT_DOORWAY: (f32, f32) = (96.0, 168.0);

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PortalMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://nvforever/door_portal.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// One door's frame: where it stands, which way is out of it, and the floor in front.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoorFrame {
    /// The door's position (game space), its height replaced by the floor's in front.
    pub origin: Vec3,
    /// Out of the door, horizontal, unit.
    pub out: Vec3,
}

impl DoorFrame {
    /// A door at `pos` whose out side holds `arrival` (where a travel through the other
    /// door puts an actor). `None` when the two are on top of each other. Out is the way to
    /// the arrival, unless `normal` (the slab's axis, either sign) says which line it is
    /// on: arrival markers stand off to the side of some doors [observed: the Prospector
    /// Saloon's back door 0x0010618E at (64, -884) turned 90°, its arrival at (133, -822)].
    pub fn new(pos: [f32; 3], arrival: [f32; 3], normal: Option<Vec3>) -> Option<DoorFrame> {
        let d = Vec3::new(arrival[0] - pos[0], arrival[1] - pos[1], 0.0);
        if d.length() <= 1.0 {
            return None;
        }
        let out = match normal.map(|n| Vec3::new(n.x, n.y, 0.0).normalize_or_zero()).filter(|n| *n != Vec3::ZERO) {
            Some(n) if n.dot(d) < 0.0 => -n,
            Some(n) => n,
            None => d.normalize(),
        };
        Some(DoorFrame { origin: Vec3::new(pos[0], pos[1], arrival[2]), out })
    }

    /// Right of the door seen from outside.
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.out.y, -self.out.x, 0.0)
    }

    /// `p` (game space) as (across to the right, out in front, up).
    pub fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.right()), d.dot(self.out), d.z)
    }
}

/// The map (game space) taking a point in front of door `from` to the matching point
/// inside door `to`'s place, [`SHIFT`] further in: walking into `from` is walking out of
/// `to`. Rotation about the vertical, then translation.
pub fn door_map(from: &DoorFrame, to: &DoorFrame) -> Affine3A {
    // `from`'s frame faces into the door, `to`'s out of it.
    let yaw_from = (-from.out).y.atan2((-from.out).x);
    let yaw_to = to.out.y.atan2(to.out.x);
    let frame = |origin: Vec3, yaw: f32| Affine3A::from_rotation_translation(Quat::from_rotation_z(yaw), origin);
    frame(to.origin + to.out * SHIFT, yaw_to) * frame(from.origin, yaw_from).inverse()
}

/// The yaw (radians, counter-clockwise seen from above) a [`door_map`] turns by.
pub fn map_yaw(m: &Affine3A) -> f32 {
    let x = m.matrix3.x_axis;
    x.y.atan2(x.x)
}

/// The same map in Bevy space.
fn to_bevy(m: &Affine3A) -> Mat4 {
    let b = game_to_bevy().to_matrix();
    b * Mat4::from(*m) * b.inverse()
}

struct Open {
    door: LoadDoor,
    back: LoadDoor,
    from: DoorFrame,
    to: DoorFrame,
    map: Affine3A,
    /// The doorway: half its width, its bottom and top (local to `from`).
    half_width: f32,
    camera: Entity,
    sun: Entity,
    quad: Entity,
}

#[derive(Resource, Default)]
pub struct Portal {
    open: Option<Open>,
    image: Option<Handle<Image>>,
    /// Layers of the parked place's entities before the portal took them.
    saved: HashMap<Entity, Option<RenderLayers>>,
    /// Door models hidden while open, with their visibility before.
    hidden: Vec<(Entity, Visibility)>,
    /// The outdoors' look, last seen outdoors (for a view out of an interior).
    outside: Option<PlaceLook>,
    /// Open this door (the one just come out of) once the travel is in.
    reopen: Option<u32>,
    last_cross: f64,
    opened: usize,
    crossings: usize,
}

pub struct DoorPortalPlugin;

impl Plugin for DoorPortalPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "door_portal.wesl");
        app.add_plugins(MaterialPlugin::<PortalMaterial>::default())
            .init_resource::<Portal>()
            .add_systems(Update, (ready_doors, open_portal, cross).chain().in_set(nvf_play::PlaySet::Input))
            .add_systems(Update, remember_outside.after(crate::world::stream))
            .add_systems(PostUpdate, adopt_new.before(bevy::camera::visibility::VisibilitySystems::CheckVisibility))
            .add_systems(PostUpdate, drive.after(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, report);
    }
}

/// The doors that can open onto their other side now: near, leading to the parked place.
fn ready_doors(
    seamless: Option<Res<Seamless>>,
    ahead: Res<DoorsAhead>,
    doors: Option<Res<LoadDoors>>,
    location: Res<nvf_play::Location>,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    mut ready: ResMut<PortalDoors>,
) {
    let on = seamless.is_some_and(|s| s.portal);
    let mut set = std::collections::HashSet::new();
    if let (true, Some(doors), Some(parked)) = (on, doors, streamer.parked.as_ref()) {
        for d in &ahead.doors {
            let to = d.travel.destination(*location);
            if parked.is(to.interior, world.world) && doors.get(d.to_door).is_some() {
                set.insert(d.door);
            }
        }
    }
    if ready.0 != set {
        ready.0 = set;
    }
}

/// The frames of door `door` and the one it leads to, and the map between them. `normal`
/// gives a door's slab axis from its drawn model ([`slab_normal`]).
fn frames(doors: &LoadDoors, door: &LoadDoor, normal: impl Fn(&LoadDoor) -> Option<Vec3>) -> Option<(LoadDoor, DoorFrame, DoorFrame, Affine3A)> {
    let back = *doors.get(door.to_door)?;
    let from = DoorFrame::new(door.pos, back.travel.feet, normal(door))?;
    let to = DoorFrame::new(back.pos, door.travel.feet, normal(&back))?;
    Some((back, from, to, door_map(&from, &to)))
}

/// The axis a door's slab faces along (game space, either sign): of the door's own two
/// horizontal axes (its z rotation), the one its drawn bounds are thinnest along. `None`
/// without drawn bounds.
fn slab_normal(rot_z: f32, visuals: &[(&GlobalTransform, &bevy::camera::primitives::Aabb)]) -> Option<Vec3> {
    if visuals.is_empty() {
        return None;
    }
    let (ax, ay) = (Vec3::new(rot_z.cos(), rot_z.sin(), 0.0), Vec3::new(-rot_z.sin(), rot_z.cos(), 0.0));
    let mut span = [(f32::MAX, f32::MIN); 2];
    for (gt, aabb) in visuals {
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        for i in 0..8 {
            let s = Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
            let p = bevy_to_game(gt.transform_point(c + h * s));
            for (k, a) in [ax, ay].iter().enumerate() {
                let v = p.dot(*a);
                span[k] = (span[k].0.min(v), span[k].1.max(v));
            }
        }
    }
    let (wx, wy) = (span[0].1 - span[0].0, span[1].1 - span[1].0);
    Some(if wx < wy { ax } else { ay })
}

/// The doorway of `door` (local to its frame): half the width, from the drawn bounds of
/// its model where known.
fn doorway(frame: &DoorFrame, visuals: &[(&GlobalTransform, &bevy::camera::primitives::Aabb)]) -> (f32, f32, f32, f32) {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for (gt, aabb) in visuals {
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        for i in 0..8 {
            let s = Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
            let l = frame.local(bevy_to_game(gt.transform_point(c + h * s)));
            lo = lo.min(l);
            hi = hi.max(l);
        }
    }
    if lo.x > hi.x || hi.x - lo.x < 16.0 || hi.z - lo.z < 32.0 {
        let (w, h) = DEFAULT_DOORWAY;
        return (-w / 2.0, w / 2.0, 0.0, h);
    }
    // The door slab's front face.
    (lo.x, hi.x, lo.z, hi.z)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn open_portal(
    mut commands: Commands,
    mut asked: MessageReader<OpenPortal>,
    seamless: Option<Res<Seamless>>,
    doors: Option<Res<LoadDoors>>,
    location: Res<nvf_play::Location>,
    streamer: Res<Streamer>,
    world: Res<WorldData>,
    mut portal: ResMut<Portal>,
    visuals: Query<(Entity, &RefVisual, &Visibility, &GlobalTransform, Option<&bevy::camera::primitives::Aabb>)>,
    children: Query<&Children>,
    layers: Query<Option<&RenderLayers>>,
    (mut images, mut meshes, mut materials, window): (ResMut<Assets<Image>>, ResMut<Assets<Mesh>>, ResMut<Assets<PortalMaterial>>, Query<&Window, With<PrimaryWindow>>),
) {
    let mut wanted = asked.read().last().map(|o| o.door);
    if wanted.is_none() {
        wanted = portal.reopen.take().filter(|_| streamer.parked.is_some());
    }
    let (Some(door), Some(doors)) = (wanted, doors) else { return };
    if !seamless.is_some_and(|s| s.portal) {
        return;
    }
    if let Some(o) = portal.open.as_ref() {
        if o.door.door != door {
            // Another door is open: it closes now, this one opens next frame (the layers
            // of the place behind go back first).
            close(&mut commands, &mut portal, &streamer);
            portal.reopen = Some(door);
        }
        return;
    }
    let Some(d) = doors.get(door).copied() else { return };
    let to = d.travel.destination(*location);
    if !streamer.parked.as_ref().is_some_and(|p| p.is(to.interior, world.world)) {
        info!("door portal: {door:08X} leads to a place not loaded: not opened");
        return;
    }
    let drawn = |id: u32| -> Vec<(&GlobalTransform, &bevy::camera::primitives::Aabb)> { visuals.iter().filter(|v| v.1 .0 == id).filter_map(|v| v.4.map(|a| (v.3, a))).collect() };
    let Some((back, from, to_frame, map)) = frames(&doors, &d, |x| slab_normal(x.rot[2], &drawn(x.door))) else {
        info!("door portal: {door:08X}: its two sides cannot be told apart: not opened");
        return;
    };
    let mine = drawn(door);
    let (x0, x1, z0, z1) = doorway(&from, &mine);
    // Hide both doors' models.
    for (e, v, vis, _, _) in &visuals {
        if v.0 == door || v.0 == back.door {
            portal.hidden.push((e, *vis));
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
    // The parked place on the portal's layer, shown.
    let roots: Vec<Entity> = streamer.parked.as_ref().map(|p| p.cells.values().copied().collect()).unwrap_or_default();
    let mut stack = roots.clone();
    while let Some(e) = stack.pop() {
        let before = layers.get(e).ok().flatten().cloned();
        let world_layer = before.as_ref().is_none_or(|l| l.intersects(&RenderLayers::layer(0)));
        if world_layer {
            let new = match &before {
                Some(l) => l.clone().without(0).with(PORTAL_LAYER),
                None => RenderLayers::layer(PORTAL_LAYER),
            };
            portal.saved.insert(e, before);
            commands.entity(e).insert(new);
        }
        if let Ok(c) = children.get(e) {
            stack.extend(c.iter());
        }
    }
    for &e in &roots {
        commands.entity(e).insert(Visibility::Inherited);
    }
    // The texture, the camera and its light.
    let size = window.single().map(|w| Extent3d { width: w.physical_width().max(1), height: w.physical_height().max(1), depth_or_array_layers: 1 }).unwrap_or(Extent3d { width: 1280, height: 720, depth_or_array_layers: 1 });
    let image = match portal.image.clone().filter(|h| images.get(h).is_some_and(|i| i.texture_descriptor.size == size)) {
        Some(h) => h,
        None => {
            let mut image = Image::new_uninit(size, TextureDimension::D2, TextureFormat::Rgba16Float, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD);
            image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
            let h = images.add(image);
            portal.image = Some(h.clone());
            h
        }
    };
    let look = match to.interior {
        Some(c) => fnv_world::lighting::cell_lighting(world.lo, world.schema, c).map(|l| SkyState::interior_look(&l)),
        None => portal.outside.clone(),
    };
    let look = look.unwrap_or_else(|| PlaceLook {
        ambient: Color::WHITE,
        ambient_brightness: crate::sky::light_unit(),
        light: Color::WHITE,
        illuminance: 0.0,
        light_rotation: Quat::IDENTITY,
        fog: crate::sky::initial_fog(),
        clear: Color::BLACK,
    });
    let camera = commands
        .spawn((
            Name::new("Door portal camera"),
            Camera3d::default(),
            Camera { order: -2, clear_color: ClearColorConfig::Custom(look.clear), ..default() },
            RenderTarget::Image(image.clone().into()),
            Hdr,
            Tonemapping::Linear,
            Msaa::Off,
            Projection::Perspective(PerspectiveProjection::default()),
            look.fog.clone(),
            AmbientLight { color: look.ambient, brightness: look.ambient_brightness, ..default() },
            RenderLayers::layer(PORTAL_LAYER),
            PortalCamera,
        ))
        .id();
    let sun = commands
        .spawn((
            Name::new("Door portal light"),
            DirectionalLight { color: look.light, illuminance: look.illuminance, shadow_maps_enabled: false, ..default() },
            Transform::from_rotation(look.light_rotation),
            RenderLayers::layer(PORTAL_LAYER),
        ))
        .id();
    // The doorway: a quad on the door's front, facing out.
    let (w, h) = (x1 - x0, z1 - z0);
    let centre = from.origin + from.right() * ((x0 + x1) / 2.0) + Vec3::Z * ((z0 + z1) / 2.0) + from.out * 1.0;
    let rot = game_to_bevy().rotation;
    // (left, up, out) is right-handed: the quad's +z faces out of the door.
    let (bx, by, bz) = (rot * -from.right(), rot * Vec3::Z, rot * from.out);
    let quad = commands
        .spawn((
            Name::new("Door portal"),
            Mesh3d(meshes.add(Rectangle::new(w, h))),
            MeshMaterial3d(materials.add(PortalMaterial { image })),
            Transform { translation: game_to_bevy().transform_point(centre), rotation: Quat::from_mat3(&Mat3::from_cols(bx, by, bz)), ..default() },
        ))
        .id();
    info!(
        "door portal: {door:08X} opens onto {} ({:.0} x {:.0} doorway; {} entities of the place behind it shown)",
        match to.interior {
            Some(c) => format!("interior {c:08X}"),
            None => format!("worldspace {:08X}", to.world),
        },
        w,
        h,
        portal.saved.len()
    );
    portal.opened += 1;
    portal.open = Some(Open { door: d, back, from, to: to_frame, map, half_width: (x1 - x0) / 2.0, camera, sun, quad });
}

/// While a door is open, meshes and lights spawned into the parked place since (an
/// actor dressed again, a dropped item) go on the portal's layer too.
#[allow(clippy::type_complexity)]
fn adopt_new(
    mut commands: Commands,
    mut portal: ResMut<Portal>,
    streamer: Res<Streamer>,
    new: Query<(Entity, Option<&RenderLayers>), Or<(Added<Mesh3d>, Added<PointLight>, Added<SpotLight>)>>,
    parents: Query<&ChildOf>,
) {
    if portal.open.is_none() || new.is_empty() {
        return;
    }
    let Some(parked) = streamer.parked.as_ref() else { return };
    let roots: std::collections::HashSet<Entity> = parked.cells.values().copied().collect();
    for (e, layers) in &new {
        if portal.saved.contains_key(&e) || !layers.is_none_or(|l| l.intersects(&RenderLayers::layer(0))) {
            continue;
        }
        let mut at = e;
        let mut inside = false;
        for _ in 0..32 {
            let Ok(p) = parents.get(at) else { break };
            at = p.parent();
            if roots.contains(&at) {
                inside = true;
                break;
            }
        }
        if inside {
            let before = layers.cloned();
            let new = match &before {
                Some(l) => l.clone().without(0).with(PORTAL_LAYER),
                None => RenderLayers::layer(PORTAL_LAYER),
            };
            portal.saved.insert(e, before);
            commands.entity(e).insert(new);
        }
    }
}

/// Close the open door: its camera, light and quad go, the doors' models come back, the
/// parked place goes back to its layers, hidden.
fn close(commands: &mut Commands, portal: &mut Portal, streamer: &Streamer) {
    let Some(o) = portal.open.take() else { return };
    for e in [o.camera, o.sun, o.quad] {
        commands.entity(e).despawn();
    }
    restore(commands, portal);
    if let Some(p) = streamer.parked.as_ref() {
        for &e in p.cells.values() {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
}

/// The door models and the parked place's layers as they were.
fn restore(commands: &mut Commands, portal: &mut Portal) {
    for (e, v) in portal.hidden.drain(..) {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.insert(v);
        }
    }
    for (e, l) in portal.saved.drain() {
        if let Ok(mut ec) = commands.get_entity(e) {
            match l {
                Some(l) => ec.insert(l),
                None => ec.remove::<RenderLayers>(),
            };
        }
    }
}

/// The player walks into the open doorway: through it (module notes); or away: it closes.
#[allow(clippy::too_many_arguments)]
fn cross(
    mut commands: Commands,
    mut portal: ResMut<Portal>,
    streamer: Res<Streamer>,
    player: Option<Res<nvf_play::player::Player>>,
    state: Option<Res<State<nvf_play::PlayState>>>,
    mut travel: MessageWriter<nvf_play::Travel>,
    mut keeps: ResMut<TravelKeeps>,
    location: Res<nvf_play::Location>,
    cams: Query<&GlobalTransform, With<PortalCamera>>,
    main: Query<&GlobalTransform, crate::fly::WorldCamera>,
    loading: Option<Res<nvf_play::loading::Loading>>,
) {
    let (Some(player), Some(o)) = (player, portal.open.as_ref()) else { return };
    if streamer.parked.is_none() || loading.as_deref().is_some_and(|l| l.active()) {
        // The place behind the door went (a travel elsewhere), or a screen is up.
        info!("door portal: {:08X} closes (the place behind it is no longer kept, or a loading screen came up)", o.door.door);
        close(&mut commands, &mut portal, &streamer);
        return;
    }
    let feet = Vec3::from_array(player.feet);
    let l = o.from.local(feet);
    if l.y.abs() > CLOSE_DISTANCE || l.x.abs() > CLOSE_DISTANCE {
        info!("door portal: {:08X} closes (the player walked off)", o.door.door);
        close(&mut commands, &mut portal, &streamer);
        return;
    }
    let now = nvf_play::seamless::now_secs();
    let playing = state.is_none_or(|s| *s.get() == nvf_play::PlayState::Playing);
    let look = Vec3::from_array(player.look_dir());
    let into = look.truncate().normalize_or_zero().dot((-o.from.out).truncate()) > 0.3;
    if !playing || now - portal.last_cross < CROSS_COOLDOWN || l.y > CROSS_DISTANCE || l.y < -CROSS_DISTANCE || l.x.abs() > o.half_width || !into {
        return;
    }
    // Through: the travel puts the player where the portal camera stood.
    let at = o.map.transform_point3(feet);
    let yaw = map_yaw(&o.map);
    let heading_deg = (player.heading - yaw).to_degrees().rem_euclid(360.0);
    let t = nvf_play::Travel { feet: at.to_array(), heading_deg, ..o.door.travel };
    if let (Ok(pc), Ok(mc)) = (cams.get(o.camera), main.single()) {
        let (pc, mc) = (bevy_to_game(pc.translation()), bevy_to_game(mc.translation()));
        debug!(
            "door portal: at the crossing the view is at ({:.0}, {:.0}, {:.0}), its map ({:.0}, {:.0}, {:.0}), the portal camera at ({:.0}, {:.0}, {:.0}); feet ({:.0}, {:.0}, {:.0}) map to ({:.0}, {:.0}, {:.0})",
            mc.x, mc.y, mc.z, o.map.transform_point3(mc).x, o.map.transform_point3(mc).y, o.map.transform_point3(mc).z, pc.x, pc.y, pc.z, feet.x, feet.y, feet.z, at.x, at.y, at.z
        );
    }
    let back = o.back.door;
    info!(
        "door portal: through {:08X} to {} at ({:.0}, {:.0}, {:.0}): no fade, no loading screen",
        o.door.door,
        match t.destination(*location).interior {
            Some(c) => format!("interior {c:08X}"),
            None => "the worldspace".to_string(),
        },
        at.x,
        at.y,
        at.z
    );
    let o = portal.open.take().expect("open");
    for e in [o.camera, o.sun, o.quad] {
        commands.entity(e).despawn();
    }
    // The place behind the door becomes the player's: its own layers back; the travel
    // shows it (`world::travel`) and hides the place left.
    restore(&mut commands, &mut portal);
    keeps.0 = Some((t, true));
    travel.write(t);
    portal.last_cross = now;
    portal.crossings += 1;
    portal.reopen = Some(back);
}

/// Outdoors: remember how the outdoors is lit, for a view out from an interior.
#[allow(clippy::type_complexity)]
fn remember_outside(
    mut portal: ResMut<Portal>,
    streamer: Res<Streamer>,
    main: Query<&DistanceFog, crate::fly::WorldCamera>,
    sun: Query<(&DirectionalLight, &GlobalTransform), With<crate::sky::Sun>>,
    ambient: Res<GlobalAmbientLight>,
    clear: Res<ClearColor>,
    mut frames: Local<u32>,
) {
    *frames += 1;
    if streamer.interior.is_some() || !streamer.settled || *frames % 30 != 0 {
        return;
    }
    let (Ok(fog), Ok((light, t))) = (main.single(), sun.single()) else { return };
    portal.outside = Some(PlaceLook {
        ambient: ambient.color,
        ambient_brightness: ambient.brightness,
        light: light.color,
        illuminance: light.illuminance,
        light_rotation: t.compute_transform().rotation,
        fog: fog.clone(),
        clear: clear.0,
    });
}

/// The portal camera follows the main view through the map, clipped at the far door.
#[allow(clippy::type_complexity)]
fn drive(
    portal: Res<Portal>,
    main: Query<(&GlobalTransform, &Projection), (crate::fly::WorldCamera, Without<PortalCamera>)>,
    mut cams: Query<(&mut Transform, &mut GlobalTransform, &mut Projection), With<PortalCamera>>,
) {
    let Some(o) = portal.open.as_ref() else { return };
    let (Ok((main_gt, Projection::Perspective(p))), Ok((mut t, mut gt, mut proj))) = (main.single(), cams.get_mut(o.camera)) else { return };
    let m = to_bevy(&o.map) * main_gt.to_matrix();
    *t = Transform::from_matrix(m);
    *gt = GlobalTransform::from(*t);
    // Keep only what lies on the room side of the far door's plane.
    let rot = game_to_bevy().rotation;
    let n = rot * o.to.out;
    let on_plane = game_to_bevy().transform_point(o.to.origin - o.to.out * 1.0);
    let view_from_world = m.inverse();
    let n_view = view_from_world.transform_vector3(n).normalize();
    let p_view = view_from_world.transform_point3(on_plane);
    let w = -n_view.dot(p_view);
    // An oblique near plane needs the camera behind it [documented: Lengyel 2005, the
    // method bevy_camera's PerspectiveProjection follows]: once the map has put the
    // camera past the far door (the player within SHIFT of this one), what lies behind
    // that door is behind the camera too, and the plain near plane is kept.
    let near_clip_plane = if w < -p.near { n_view.extend(w) } else { Vec4::new(0.0, 0.0, -1.0, -p.near) };
    *proj = Projection::Perspective(PerspectiveProjection { near_clip_plane, ..p.clone() });
}

/// Marks the portal's camera.
#[derive(Component)]
pub struct PortalCamera;

fn report(mut exit: MessageReader<AppExit>, seamless: Option<Res<Seamless>>, portal: Res<Portal>) {
    if exit.read().next().is_none() || !seamless.is_some_and(|s| s.portal) {
        return;
    }
    info!("door portal summary: {} door(s) opened onto their other side, {} walked through", portal.opened, portal.crossings);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two doors facing each other's arrival: an exterior door at (1000, 0) whose out side is
    // +x (the travel back arrives at (1100, 0)), and an interior door at (0, 500) whose
    // inside is -y (arrivals in the room at (0, 400)).
    fn pair() -> (DoorFrame, DoorFrame) {
        let from = DoorFrame::new([1000.0, 0.0, 8000.0], [1100.0, 0.0, 8000.0], None).unwrap();
        let to = DoorFrame::new([0.0, 500.0, 3400.0], [0.0, 400.0, 3456.0], None).unwrap();
        (from, to)
    }

    #[test]
    fn walking_into_one_door_is_walking_out_of_the_other() {
        let (from, to) = pair();
        let m = door_map(&from, &to);
        // At the door's plane: SHIFT into the room, floor to floor.
        let at = m.transform_point3(Vec3::new(1000.0, 0.0, 8000.0));
        assert!((at - Vec3::new(0.0, 500.0 - SHIFT, 3456.0)).length() < 1e-2, "{at}");
        // A step outside (in front of the door) is a step less into the room.
        let at = m.transform_point3(Vec3::new(1030.0, 0.0, 8000.0));
        assert!((at - Vec3::new(0.0, 500.0 - SHIFT + 30.0, 3456.0)).length() < 1e-2, "{at}");
        // Heading into the door (-x) is heading into the room (-y): a quarter turn.
        let dir = m.transform_vector3(Vec3::NEG_X);
        assert!((dir - Vec3::NEG_Y).length() < 1e-5, "{dir}");
        assert!((map_yaw(&m) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn crossing_lands_clear_of_the_far_door() {
        let (from, to) = pair();
        let m = door_map(&from, &to);
        let feet = Vec3::new(1000.0 + CROSS_DISTANCE, 0.0, 8000.0);
        let at = to.local(m.transform_point3(feet));
        assert!(at.y >= SHIFT - CROSS_DISTANCE - 1e-3, "{at}");
    }

    #[test]
    fn a_door_on_its_arrival_has_no_frame() {
        assert!(DoorFrame::new([5.0, 5.0, 0.0], [5.0, 5.5, 0.0], None).is_none());
    }

    #[test]
    fn the_slab_says_which_line_out_is_on_and_the_arrival_which_way() {
        // The saloon's back door: arrival off to the north-east, slab facing east-west.
        let f = DoorFrame::new([64.0, -884.0, 3456.0], [133.0, -822.0, 3456.0], Some(Vec3::NEG_X)).unwrap();
        assert!((f.out - Vec3::X).length() < 1e-6, "{}", f.out);
        let f = DoorFrame::new([64.0, -884.0, 3456.0], [133.0, -822.0, 3456.0], None).unwrap();
        assert!(f.out.x > 0.7 && f.out.y > 0.6, "without the slab: towards the arrival");
    }
}
