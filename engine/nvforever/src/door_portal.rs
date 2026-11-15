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

/// The layer of the parked place's meshes that lie wholly behind the far door, which the
/// door's view leaves out (seen from behind the far door's wall, they would stand in the
/// doorway) [design: in place of an oblique near plane, whose skewed far plane cut away
/// the saloon's ceiling fan and light shafts from the view].
pub const PORTAL_BEHIND_LAYER: usize = 21;

/// The player's feet this near the door's plane (game units), inside its width and looking
/// into it, go through [engineering choice: both doors' collision is taken away while
/// open, so the player walks into the doorway; a door set on a wall with no opening
/// behind it stops the 22-unit capsule about this far out].
pub const CROSS_DISTANCE: f32 = 24.0;

/// How much further in than the other door the map puts a point (game units)
/// [engineering choice: [`CROSS_DISTANCE`] plus room for the capsule, so the travel lands
/// clear of the far door].
pub const SHIFT: f32 = CROSS_DISTANCE + 32.0;

/// How far out from a door its floor is measured (game units) [engineering choice: past
/// the slab and its frame, where the player stands to go through].
const THRESHOLD: f32 = 32.0;

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
    /// x = 1: drawn flat magenta (the probe test aid's mask of the doorway).
    #[uniform(2)]
    pub debug: Vec4,
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
    /// The doorway: half its width and height, its centre (game space).
    half_width: f32,
    half_height: f32,
    quad_centre: Vec3,
    camera: Entity,
    sun: Entity,
    quad: Entity,
}

#[derive(Resource, Default)]
pub struct Portal {
    open: Option<Open>,
    image: Option<Handle<Image>>,
    /// Layers of the parked place's entities before the portal took them, and the ones the
    /// portal gave them.
    saved: HashMap<Entity, (Option<RenderLayers>, RenderLayers)>,
    /// Door models hidden while open, with their visibility before.
    hidden: Vec<(Entity, Visibility)>,
    /// The outdoors' look, last seen outdoors (for a view out of an interior).
    outside: Option<PlaceLook>,
    /// The sky's parts drawn outdoors then: the dome and the cloud layers showing.
    outside_sky: Vec<Entity>,
    /// Copies of them drawn in the door's view out of an interior ([`PortalSky`]).
    sky: Vec<Entity>,
    /// Open this door (the one just come out of) once the travel is in.
    reopen: Option<u32>,
    last_cross: f64,
    opened: usize,
    crossings: usize,
    /// Test aid `NVF_PLAY_TEST_PORTAL_PROBE` ([`probe`]).
    probe: Probe,
    /// Doors whose collision is to be given back (`RefChanged`, [`give_back`]).
    give_back: Vec<u32>,
}

/// Test aid `NVF_PLAY_TEST_PORTAL_PROBE=<dir>[:<count>]`: for the first `count` (1)
/// doors the player opens and walks up to, a screenshot through the doorway once the
/// player is within [`PROBE_DISTANCE`] of it (`probe<n>-a.png`), the crossing forced two
/// frames later, and screenshots 3 and 30 frames after it (`-b`, `-c`); the doorway's
/// rectangle on screen is logged. A frame through the door and the first after it should
/// be the same picture. Not a game behaviour.
#[derive(Default)]
struct Probe {
    dir: Option<std::path::PathBuf>,
    left: u32,
    n: u32,
    /// The frame the mask was asked for; screenshot A; the frame the crossing went.
    mask_at: Option<u32>,
    shot_at: Option<u32>,
    crossed_at: Option<u32>,
    /// Hold the player's own crossing while probing; force it.
    hold: bool,
    force: bool,
    /// The door last opened, whose player was placed before it (once per door).
    placed_for: Option<Entity>,
}

/// The probe's screenshot through the doorway is taken this near it (game units).
const PROBE_DISTANCE: f32 = CROSS_DISTANCE + 12.0;

fn probe_from_env() -> Probe {
    let Ok(v) = std::env::var("NVF_PLAY_TEST_PORTAL_PROBE") else { return Probe::default() };
    let (dir, count) = match v.rsplit_once(':').filter(|(_, c)| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_digit())) {
        Some((d, c)) => (d.to_string(), c.parse().unwrap_or(1)),
        None => (v.clone(), 1),
    };
    info!("test aid NVF_PLAY_TEST_PORTAL_PROBE: {count} door(s) probed into {dir}");
    Probe { dir: Some(dir.into()), left: count, ..default() }
}

pub struct DoorPortalPlugin;

impl Plugin for DoorPortalPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "door_portal.wesl");
        app.add_plugins(MaterialPlugin::<PortalMaterial>::default())
            .insert_resource(Portal { probe: probe_from_env(), ..default() })
            .add_systems(Update, (ready_doors, open_portal, probe, cross).chain().in_set(nvf_play::PlaySet::Input))
            .add_systems(Update, (remember_outside, ease_grade).after(crate::world::stream))
            .add_systems(PostUpdate, (adopt_new, reassert).chain().before(bevy::camera::visibility::VisibilitySystems::CheckVisibility))
            .init_resource::<crate::imagespace::PortalGradeSource>()
            .add_systems(PostUpdate, (drive, doorway_grade).chain().after(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, (report, debug_refs))
            .add_systems(Update, give_back.after(cross));
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
    // Out of a door: the way the travel arriving at it faces (an arrival marker faces away
    // from its door [observed: Goodsprings' doors]), on the door's own axes; else its
    // slab's thin axis ([`slab_normal`]).
    let out = |d: &LoadDoor, arriving: &LoadDoor| Some(snap_to_axes(heading_dir(arriving.travel.heading_deg), d.rot[2])).or_else(|| normal(d));
    let from = DoorFrame::new(door.pos, back.travel.feet, out(door, &back))?;
    let to = DoorFrame::new(back.pos, door.travel.feet, out(&back, door))?;
    Some((back, from, to, door_map(&from, &to)))
}

/// A compass heading (degrees, 0 north, clockwise) as a direction (game space).
fn heading_dir(deg: f32) -> Vec3 {
    let r = deg.to_radians();
    Vec3::new(r.sin(), r.cos(), 0.0)
}

/// `v` turned onto the nearest of the four horizontal axes of a reference turned `rot_z`
/// (radians) about the vertical.
fn snap_to_axes(v: Vec3, rot_z: f32) -> Vec3 {
    let (ax, ay) = (Vec3::new(rot_z.cos(), rot_z.sin(), 0.0), Vec3::new(-rot_z.sin(), rot_z.cos(), 0.0));
    [ax, -ax, ay, -ay].into_iter().max_by(|a, b| a.dot(v).total_cmp(&b.dot(v))).unwrap_or(v)
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
/// `frame` moved to the middle of the door's drawn slab, across it and through it (its
/// height stays the floor's): a door model's origin may be at a hinge or off its middle.
fn centre_on_slab(frame: &DoorFrame, visuals: &[(&GlobalTransform, &bevy::camera::primitives::Aabb)]) -> DoorFrame {
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
    if lo.x > hi.x || hi.x - lo.x < 16.0 {
        return *frame;
    }
    let mid = (lo + hi) / 2.0;
    DoorFrame { origin: frame.origin + frame.right() * mid.x + frame.out * mid.y, out: frame.out }
}

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
    (room_hidden, parked_actors): (Query<(), With<crate::rooms::RoomHidden>>, Query<Entity, With<crate::world::ParkedActor>>),
    bounds: Query<(&GlobalTransform, &bevy::camera::primitives::Aabb)>,
    (lod, sky_parts, mut grade_source): (Res<crate::lod::LodState>, Query<(&Mesh3d, &MeshMaterial3d<crate::sky::SkyMaterial>, &Transform), Without<PortalSky>>, ResMut<crate::imagespace::PortalGradeSource>),
    (mut images, mut meshes, mut materials, window): (ResMut<Assets<Image>>, ResMut<Assets<Mesh>>, ResMut<Assets<PortalMaterial>>, Query<&Window, With<PrimaryWindow>>),
    (mut phys, sky, main): (Option<ResMut<nvf_play::phys::PhysState>>, Res<SkyState>, Query<(Option<&bevy::light::ShadowFilteringMethod>, &Msaa, Has<bevy::core_pipeline::prepass::DepthPrepass>, Has<bevy::core_pipeline::prepass::NormalPrepass>, Has<bevy::core_pipeline::prepass::MotionVectorPrepass>), crate::fly::WorldCamera>),
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
    let Some((back, from, to_frame, _)) = frames(&doors, &d, |x| slab_normal(x.rot[2], &drawn(x.door))) else {
        info!("door portal: {door:08X}: its two sides cannot be told apart: not opened");
        return;
    };
    // On the doors' drawn slabs: a door's origin is not always its doorway's middle.
    let mut from = centre_on_slab(&from, &drawn(door));
    let mut to_frame = centre_on_slab(&to_frame, &drawn(back.door));
    // Floor to floor: the ground under each door's arrival marker, which may stand above
    // it [observed: Doc Mitchell's house, both markers about 52 units up]; physics puts
    // the player there, and the map must too or the view jumps at the crossing.
    // The floor at the threshold, just out from each door (not under its marker: Doc
    // Mitchell's outer marker stands off his raised porch).
    if let Some(p) = phys.as_deref_mut() {
        for (f, x) in [(&mut from, &d), (&mut to_frame, &back)] {
            let at = f.origin + f.out * THRESHOLD;
            let top = x.pos[2].max(f.origin.z) + 96.0;
            if let Some(hit) = p.ground_below([at.x, at.y, top], 0.0, 320.0) {
                f.origin.z = hit.point[2];
            }
        }
    }
    let map = door_map(&from, &to_frame);
    let mine = drawn(door);
    for (x, arriving, f) in [(&d, &back, &from), (&back, &d, &to_frame)] {
        info!(
            "door portal: door {:08X} at ({:.0}, {:.0}) turned {:.0}°, arrival at it ({:.0}, {:.0}) facing {:.0}°, slab axis {:?}: out ({:.2}, {:.2})",
            x.door, x.pos[0], x.pos[1], x.rot[2].to_degrees(), arriving.travel.feet[0], arriving.travel.feet[1], arriving.travel.heading_deg,
            slab_normal(x.rot[2], &drawn(x.door)).map(|n| (n.x * 100.0).round() / 100.0), f.out.x, f.out.y
        );
    }
    let (x0, x1, z0, z1) = doorway(&from, &mine);
    // Both doors' collision goes while open (given back on closing: `RefChanged`).
    if let Some(p) = phys.as_deref_mut() {
        p.world.remove_ref(door);
        p.world.remove_ref(back.door);
    }
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
    // Actors drawn standing in the place behind (`residents.rs`).
    stack.extend(parked_actors.iter());
    while let Some(e) = stack.pop() {
        let culled = room_hidden.contains(e);
        let before = if culled { None } else { layers.get(e).ok().flatten().cloned() };
        if culled {
            // Hidden by an interior's room culling when it was left: drawn through the door.
            commands.entity(e).remove::<crate::rooms::RoomHidden>();
        }
        let world_layer = world_drawn(before.as_ref());
        if world_layer {
            let behind = bounds.get(e).is_ok_and(|(gt, aabb)| wholly_behind(&to_frame, gt, aabb));
            let new = portal_layers(before.as_ref(), behind);
            portal.saved.insert(e, (before, new.clone()));
            commands.entity(e).insert(new);
        }
        if let Ok(c) = children.get(e) {
            stack.extend(c.iter());
        }
    }
    for &e in &roots {
        commands.entity(e).insert(Visibility::Inherited);
    }
    // Out of an interior: the distant land, kept hidden (`LodState::park`), and the sky.
    if to.interior.is_none() && streamer.interior.is_some() {
        let mut stack = lod.entities();
        for &e in &stack {
            portal.hidden.push((e, Visibility::Hidden));
            commands.entity(e).insert(Visibility::Inherited);
        }
        while let Some(e) = stack.pop() {
            let before = layers.get(e).ok().flatten().cloned();
            if world_drawn(before.as_ref()) {
                let new = portal_layers(before.as_ref(), false);
                portal.saved.insert(e, (before, new.clone()));
                commands.entity(e).insert(new);
            }
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
        let parts: Vec<Entity> = portal.outside_sky.clone();
        for e in parts {
            let Ok((mesh, material, t)) = sky_parts.get(e) else { continue };
            let mut t = *t;
            if t.scale.x <= 0.0 {
                t = crate::sky::sky_transform(t.translation);
            }
            let copy = commands
                .spawn((Name::new("Door portal sky"), PortalSky, Mesh3d(mesh.0.clone()), MeshMaterial3d(material.0.clone()), t, RenderLayers::layer(PORTAL_LAYER), bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver))
                .id();
            portal.sky.push(copy);
        }
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
    // The doorway is graded as the place it shows (`doorway_grade`).
    grade_source.0 = Some(match to.interior {
        Some(c) if sky.grade => SkyState::cell_grade(world.lo, world.schema, c).unwrap_or_default(),
        Some(_) => Default::default(),
        None => sky.exterior_cinematic(),
    });
    let look = look.unwrap_or_else(|| PlaceLook {
        ambient: Color::WHITE,
        ambient_brightness: crate::sky::light_unit(),
        light: Color::WHITE,
        illuminance: 0.0,
        light_rotation: Quat::IDENTITY,
        fog: crate::sky::initial_fog(),
        clear: Color::BLACK,
    });
    debug!(
        "door portal: the view's light: ambient {:?} x{:.0}, light {:?} {:.0}, fog {:?} {:?}",
        look.ambient.to_linear(), look.ambient_brightness, look.light.to_linear(), look.illuminance, look.fog.color.to_linear(), look.fog.falloff
    );
    info!("door portal: the view's light: ambient x{:.0}, light {:.0}", look.ambient_brightness, look.illuminance);
    let camera = commands
        .spawn((
            Name::new("Door portal camera"),
            Camera3d::default(),
            Camera { order: -2, clear_color: ClearColorConfig::Custom(look.clear), ..default() },
            RenderTarget::Image(image.clone().into()),
            Hdr,
            Tonemapping::None,
            Projection::Perspective(PerspectiveProjection::default()),
            look.fog.clone(),
            AmbientLight { color: look.ambient, brightness: look.ambient_brightness, ..default() },
            RenderLayers::layer(PORTAL_LAYER),
            PortalCamera,
        ))
        .id();
    // As the main view (`graphics.rs`): its shadow filter, so its shadows look the same,
    // and its multisampling and prepasses, so the pipelines the door's view compiles for
    // the place behind it are the ones the main view needs once the player is through
    // (a pipeline's key holds them): otherwise the place is drawn without most of its
    // meshes until theirs are compiled, seconds after the crossing.
    if let Ok((filter, msaa, depth, normal, motion)) = main.single() {
        let mut e = commands.entity(camera);
        e.insert(*msaa);
        if let Some(f) = filter {
            e.insert(*f);
        }
        if depth {
            e.insert(bevy::core_pipeline::prepass::DepthPrepass);
        }
        if normal {
            e.insert(bevy::core_pipeline::prepass::NormalPrepass);
        }
        if motion {
            e.insert(bevy::core_pipeline::prepass::MotionVectorPrepass);
        }
    }
    let sun = commands
        .spawn((
            Name::new("Door portal light"),
            DirectionalLight { color: look.light, illuminance: look.illuminance, shadow_maps_enabled: sky.shadows, ..default() },
            Transform::from_rotation(look.light_rotation),
            // Its shadow casters are the entities on its layers: those behind the far door
            // (the building the door is in) still shade what the door shows.
            RenderLayers::from_layers(&[PORTAL_LAYER, PORTAL_BEHIND_LAYER]),
            crate::sky::shadow_cascades(sky.shadow_distance, sky.shadow_reach, sky.shadow_cascades),
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
            MeshMaterial3d(materials.add(PortalMaterial { image, debug: Vec4::ZERO })),
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
    portal.open = Some(Open { door: d, back, from, to: to_frame, map, half_width: (x1 - x0) / 2.0, half_height: (z1 - z0) / 2.0, quad_centre: centre, camera, sun, quad });
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
    (marked, parked_actors, children, layers): (Query<Entity, Added<crate::world::ParkedActor>>, Query<Entity, With<crate::world::ParkedActor>>, Query<&Children>, Query<Option<&RenderLayers>>),
) {
    if portal.open.is_none() {
        return;
    }
    // Actors come to stand in the place behind since the door opened: their bodies too.
    let mut stack: Vec<Entity> = marked.iter().collect();
    while let Some(e) = stack.pop() {
        if !portal.saved.contains_key(&e) {
            let before = layers.get(e).ok().flatten().cloned();
            if world_drawn(before.as_ref()) {
                let new = portal_layers(before.as_ref(), false);
                portal.saved.insert(e, (before, new.clone()));
                commands.entity(e).insert(new);
            }
        }
        if let Ok(c) = children.get(e) {
            stack.extend(c.iter());
        }
    }
    if new.is_empty() {
        return;
    }
    let Some(parked) = streamer.parked.as_ref() else { return };
    let roots: std::collections::HashSet<Entity> = parked.cells.values().copied().chain(parked_actors.iter()).collect();
    for (e, layers) in &new {
        if portal.saved.contains_key(&e) || !world_drawn(layers) {
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
            let new = portal_layers(before.as_ref(), false);
            portal.saved.insert(e, (before, new.clone()));
            commands.entity(e).insert(new);
        }
    }
}

/// While a door is open, the place behind it stays on the portal's layer: another system
/// that sets or takes away render layers (an interior's room culling) is undone.
fn reassert(mut commands: Commands, portal: Res<Portal>, layers: Query<Option<&RenderLayers>>) {
    if portal.open.is_none() {
        return;
    }
    for (&e, (_, want)) in &portal.saved {
        if let Ok(now) = layers.get(e) {
            if now != Some(want) {
                commands.entity(e).insert(want.clone());
            }
        }
    }
}

/// The layers an entity of the parked place has while a door shows it: the portal's in
/// place of the world's (and of the main view's own water and decal layers), or,
/// `behind` the far door, one the door's view leaves out.
fn portal_layers(before: Option<&RenderLayers>, behind: bool) -> RenderLayers {
    let layer = if behind { PORTAL_BEHIND_LAYER } else { PORTAL_LAYER };
    match before {
        Some(l) => l.clone().without(0).without(crate::reflection::WATER_LAYER).without(crate::decals::DECAL_LAYER).with(layer),
        None => RenderLayers::layer(layer),
    }
}

/// Whether an entity is drawn by the main view as part of the world: no layers (the
/// world's), or the world's, water's or decals' layer.
fn world_drawn(layers: Option<&RenderLayers>) -> bool {
    layers.is_none_or(|l| l.intersects(&RenderLayers::from_layers(&[0, crate::reflection::WATER_LAYER, crate::decals::DECAL_LAYER])))
}

/// Whether a mesh's bounds lie wholly on the far side of a door from its out side
/// (behind its wall), by more than a couple of units.
fn wholly_behind(frame: &DoorFrame, gt: &GlobalTransform, aabb: &bevy::camera::primitives::Aabb) -> bool {
    let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
    (0..8).all(|i| {
        let s = Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
        let p = bevy_to_game(gt.transform_point(c + h * s));
        (p - frame.origin).dot(frame.out) < -2.0
    })
}

/// Close the open door: its camera, light and quad go, the doors' models come back, the
/// parked place goes back to its layers, hidden.
fn close(commands: &mut Commands, portal: &mut Portal, streamer: &Streamer) {
    let Some(o) = portal.open.take() else { return };
    portal.give_back.extend([o.door.door, o.back.door]);
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

/// The door models and the parked place's layers as they were; the sky's copies go.
fn restore(commands: &mut Commands, portal: &mut Portal) {
    for e in portal.sky.drain(..) {
        commands.entity(e).despawn();
    }
    for (e, v) in portal.hidden.drain(..) {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.insert(v);
        }
    }
    for (e, (l, _)) in portal.saved.drain() {
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
    (mut travel, mut crossed): (MessageWriter<nvf_play::Travel>, MessageWriter<nvf_play::seamless::Crossed>),
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
    let half_width = o.half_width;
    let forced = std::mem::take(&mut portal.probe.force);
    if !forced && (portal.probe.hold || !playing || now - portal.last_cross < CROSS_COOLDOWN || l.y > CROSS_DISTANCE || l.y < -CROSS_DISTANCE || l.x.abs() > half_width || !into) {
        return;
    }
    let o = portal.open.as_ref().expect("open");
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
    portal.give_back.extend([o.door.door, o.back.door]);
    for e in [o.camera, o.sun, o.quad] {
        commands.entity(e).despawn();
    }
    // The place behind the door becomes the player's: its own layers back; the travel
    // shows it (`world::travel`) and hides the place left.
    restore(&mut commands, &mut portal);
    keeps.0 = Some((t, true));
    travel.write(t);
    crossed.write(nvf_play::seamless::Crossed { door: o.door.door });
    portal.last_cross = now;
    portal.crossings += 1;
    portal.reopen = Some(back);
}

/// Doors closed or gone through: their collision back (`nvf_play::phys`'s `RefChanged`).
fn give_back(mut portal: ResMut<Portal>, mut changed: MessageWriter<nvf_play::RefChanged>) {
    for d in portal.give_back.drain(..) {
        changed.write(nvf_play::RefChanged::new(d));
    }
}

/// The probe test aid ([`Probe`]).
#[allow(clippy::too_many_arguments)]
fn probe(
    mut commands: Commands,
    mut portal: ResMut<Portal>,
    mut materials: ResMut<Assets<PortalMaterial>>,
    quads: Query<&MeshMaterial3d<PortalMaterial>>,
    frames: Res<bevy::diagnostic::FrameCount>,
    player: Option<ResMut<nvf_play::player::Player>>,
    main: Query<(&Camera, &GlobalTransform), crate::fly::WorldCamera>,
    mut phys: Option<ResMut<nvf_play::phys::PhysState>>,
    (fogs, suns, ambient, sky): (Query<&DistanceFog, crate::fly::WorldCamera>, Query<&DirectionalLight, With<crate::sky::Sun>>, Res<GlobalAmbientLight>, Res<SkyState>),
) {
    let Some(dir) = portal.probe.dir.clone() else { return };
    let frame = frames.0;
    let shot = |commands: &mut Commands, name: String| {
        let path = dir.join(name);
        if let (Ok(fog), Ok(l)) = (fogs.single(), suns.single()) {
            info!(
                "test aid: portal probe view: fog {:?} {:?}, ambient {:?} x{:.0}, sun {:?} {:.0}, indoors {}, grade ease {:?}",
                fog.color.to_linear(), fog.falloff, ambient.color.to_linear(), ambient.brightness, l.color.to_linear(), l.illuminance, sky.interior.is_some(), sky.grade_ease.as_ref().map(|g| g.1)
            );
        }
        info!("test aid: portal probe screenshot {}", path.display());
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
    };
    let n = portal.probe.n;
    if let Some(at) = portal.probe.crossed_at {
        if frame == at + 3 {
            shot(&mut commands, format!("probe{n}-b.png"));
        } else if frame >= at + 30 {
            shot(&mut commands, format!("probe{n}-c.png"));
            portal.probe.crossed_at = None;
            portal.probe.n += 1;
        }
        return;
    }
    let set_mask = |materials: &mut Assets<PortalMaterial>, portal: &Portal, on: bool| {
        if let Some(mut m) = portal.open.as_ref().and_then(|o| quads.get(o.quad).ok()).and_then(|h| materials.get_mut(&h.0)) {
            m.debug = Vec4::new(if on { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0);
        }
    };
    if let Some(at) = portal.probe.mask_at {
        if frame >= at + 2 {
            set_mask(&mut materials, &portal, false);
            portal.probe.mask_at = None;
            portal.probe.shot_at = Some(frame + 2);
        }
        return;
    }
    if let Some(at) = portal.probe.shot_at {
        if frame == at {
            shot(&mut commands, format!("probe{n}-a.png"));
            return;
        }
        if frame >= at + 3 {
            portal.probe.force = true;
            portal.probe.hold = false;
            portal.probe.shot_at = None;
            portal.probe.crossed_at = Some(frame);
        }
        return;
    }
    let (Some(o), Some(mut player)) = (portal.open.as_ref(), player) else { return };
    if portal.probe.left == 0 {
        return;
    }
    // Stand the player straight out from the door, facing it, once per door opened: the
    // test walks it in from there.
    if portal.probe.placed_for != Some(o.camera) {
        // On the door's arrival spot (where its partner's travel puts the player: clear
        // ground the level's author chose), facing the middle of the doorway.
        let at = Vec3::from_array(o.back.travel.feet);
        let into = (o.quad_centre - at).with_z(0.0).normalize_or(-o.from.out);
        if let Some(p) = phys.as_deref_mut() {
            p.place_player(at.to_array(), 120.0, 300.0);
            player.feet = p.player.as_ref().map_or(at.to_array(), |c| c.feet);
        }
        player.heading = into.x.atan2(into.y);
        player.pitch = 0.0;
        info!("test aid: portal probe {n}: the player stands 90 units out from door {:08X}, facing it", o.door.door);
        let camera = o.camera;
        portal.probe.placed_for = Some(camera);
        return;
    }
    let l = o.from.local(Vec3::from_array(player.feet));
    let look = Vec3::from_array(player.look_dir());
    let into = look.truncate().normalize_or_zero().dot((-o.from.out).truncate()) > 0.5;
    if l.y > PROBE_DISTANCE || l.y < 0.0 || l.x.abs() > o.half_width || !into {
        return;
    }
    // The doorway on screen.
    if let Ok((cam, gt)) = main.single() {
        let (c, r, u) = (o.quad_centre, o.from.right(), Vec3::Z);
        let (w, h) = (o.half_width, o.half_height);
        let corners = [c - r * w - u * h, c + r * w - u * h, c - r * w + u * h, c + r * w + u * h];
        let size = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 800.0));
        let pts: Vec<Vec2> = corners
            .iter()
            .filter_map(|p| cam.world_to_ndc(gt, game_to_bevy().transform_point(*p)))
            .map(|n| Vec2::new((n.x.clamp(-1.0, 1.0) + 1.0) / 2.0 * size.x, (1.0 - n.y.clamp(-1.0, 1.0)) / 2.0 * size.y))
            .collect();
        if pts.len() == 4 {
            let (lo, hi) = pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
            info!("test aid: portal probe {n}: door {:08X}, doorway on screen {:.0} {:.0} {:.0} {:.0}", o.door.door, lo.x, lo.y, hi.x, hi.y);
        }
    }
    shot(&mut commands, format!("probe{n}-mask.png"));
    set_mask(&mut materials, &portal, true);
    portal.probe.left -= 1;
    portal.probe.hold = true;
    portal.probe.mask_at = Some(frame);
}

/// Seconds the grade takes to ease from the place left to the place come into
/// [engineering choice: about the eye adaptation's own pace].
pub const GRADE_EASE_SECS: f32 = 1.5;

fn ease_grade(time: Res<Time<Real>>, mut sky: ResMut<SkyState>) {
    let Some((_, t)) = sky.grade_ease.as_mut() else { return };
    *t += time.delta_secs() / GRADE_EASE_SECS;
    if *t >= 1.0 {
        sky.grade_ease = None;
    }
}

/// Outdoors: remember how the outdoors is lit, for a view out from an interior.
#[allow(clippy::type_complexity)]
fn remember_outside(
    mut portal: ResMut<Portal>,
    streamer: Res<Streamer>,
    main: Query<&DistanceFog, crate::fly::WorldCamera>,
    sun: Query<(&DirectionalLight, &GlobalTransform), With<crate::sky::Sun>>,
    ambient: Res<GlobalAmbientLight>,
    sky_parts: Query<(Entity, &Visibility), Or<(With<crate::sky::SkyDome>, With<crate::sky::CloudLayer>)>>,
    mut frames: Local<u32>,
) {
    *frames += 1;
    if streamer.interior.is_some() || !streamer.settled || *frames % 30 != 0 {
        return;
    }
    let (Ok(fog), Ok((light, t))) = (main.single(), sun.single()) else { return };
    portal.outside_sky = sky_parts.iter().filter(|(_, v)| **v != Visibility::Hidden).map(|(e, _)| e).collect();
    portal.outside = Some(PlaceLook {
        ambient: ambient.color,
        ambient_brightness: ambient.brightness,
        light: light.color,
        illuminance: light.illuminance,
        light_rotation: t.compute_transform().rotation,
        // Past everything drawn the outdoors fade into their fog: the sky is not drawn in
        // the door's view, so its background is the fog's colour at the horizon.
        fog: fog.clone(),
        clear: fog.color.with_alpha(1.0),
    });
}

/// The portal camera follows the main view through the map, clipped at the far door.
#[allow(clippy::type_complexity)]
fn drive(
    portal: Res<Portal>,
    main: Query<(&GlobalTransform, &Projection), (crate::fly::WorldCamera, Without<PortalCamera>)>,
    mut cams: Query<(&mut Transform, &mut GlobalTransform, &mut Projection), With<PortalCamera>>,
    mut skies: Query<(&mut Transform, &mut GlobalTransform), (With<PortalSky>, Without<PortalCamera>, Without<crate::fly::FlyCam>)>,
) {
    let Some(o) = portal.open.as_ref() else { return };
    let (Ok((main_gt, Projection::Perspective(p))), Ok((mut t, mut gt, mut proj))) = (main.single(), cams.get_mut(o.camera)) else { return };
    let m = to_bevy(&o.map) * main_gt.to_matrix();
    *t = Transform::from_matrix(m);
    *gt = GlobalTransform::from(*t);
    for (mut st, mut sgt) in &mut skies {
        st.translation = t.translation;
        *sgt = GlobalTransform::from(*st);
    }
    // Keep only what lies on the room side of the far door's plane.
    let rot = game_to_bevy().rotation;
    let n = rot * o.to.out;
    let on_plane = game_to_bevy().transform_point(o.to.origin - o.to.out * 1.0);
    let view_from_world = m.inverse();
    let n_view = view_from_world.transform_vector3(n).normalize();
    let p_view = view_from_world.transform_point3(on_plane);
    let _ = (n_view, p_view);
    // No oblique near plane: what lies behind the far door is left out by layer
    // ([`PORTAL_BEHIND_LAYER`]).
    // Not the main view's far plane: indoors it is the interior's fog distance
    // (`sky::interior_clip`), which would cut the outdoors short.
    *proj = Projection::Perspective(PerspectiveProjection { near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -p.near), far: crate::sky::OUTDOOR_FAR, ..p.clone() });
}

/// A copy of a part of the sky (dome, cloud layer) for the door's view out of an
/// interior: indoors the sky itself is not drawn. Its colours stay as the sky was last
/// seen outdoors [design: the sky's material is updated only outdoors].
#[derive(Component)]
pub struct PortalSky;

/// The doorway's outline on screen for the image-space grade, which grades what the door
/// shows as its own place (`imagespace.rs`, `ImageSpaceGrade::portal_quad`): crossing,
/// the place's grade takes the whole screen with no change to what was seen through the
/// door. A corner behind the view (the player in the doorway): the whole screen.
fn doorway_grade(portal: Res<Portal>, mut source: ResMut<crate::imagespace::PortalGradeSource>, mut main: Query<(&Camera, &GlobalTransform, &mut crate::imagespace::ImageSpaceGrade), crate::fly::WorldCamera>) {
    let Ok((cam, gt, mut grade)) = main.single_mut() else { return };
    let Some(o) = portal.open.as_ref() else {
        if source.0.is_some() {
            source.0 = None;
        }
        return;
    };
    let (c, r, u) = (o.quad_centre, o.from.right(), Vec3::Z);
    let (w, h) = (o.half_width, o.half_height);
    let corners = [c - r * w - u * h, c + r * w - u * h, c + r * w + u * h, c - r * w + u * h];
    let uv: Vec<Vec2> = corners.iter().filter_map(|p| cam.world_to_ndc(gt, game_to_bevy().transform_point(*p)).filter(|n| n.z > 0.0)).map(|n| Vec2::new((n.x + 1.0) / 2.0, (1.0 - n.y) / 2.0)).collect();
    let eye = bevy_to_game(gt.translation());
    let ahead = bevy_to_game(gt.translation() + gt.forward().as_vec3()) - eye;
    let at_door = o.from.local(eye).y.abs() < CROSS_DISTANCE * 3.0 && ahead.truncate().normalize_or_zero().dot((-o.from.out).truncate()) > 0.3;
    let quad = if uv.len() == 4 {
        Mat4::from_cols(uv[0].extend(0.0).extend(0.0), uv[1].extend(0.0).extend(0.0), uv[2].extend(0.0).extend(0.0), uv[3].extend(0.0).extend(0.0))
    } else if !at_door {
        grade.portal_on = Vec4::ZERO;
        return;
    } else {
        let big = [Vec2::new(-1.0, -1.0), Vec2::new(2.0, -1.0), Vec2::new(2.0, 2.0), Vec2::new(-1.0, 2.0)];
        Mat4::from_cols(big[0].extend(0.0).extend(0.0), big[1].extend(0.0).extend(0.0), big[2].extend(0.0).extend(0.0), big[3].extend(0.0).extend(0.0))
    };
    grade.portal_quad = quad;
    grade.portal_on = Vec4::X;
}

/// Marks the portal's camera.
#[derive(Component)]
pub struct PortalCamera;

/// Diagnostic `NVF_PORTAL_DEBUG_REFS=<hex>,...`: 60 frames after a door opens, log every
/// entity of those references: its layers and visibility.
#[allow(clippy::type_complexity)]
fn debug_refs(
    portal: Res<Portal>,
    mut since: Local<(Option<Entity>, u32)>,
    q: Query<(Entity, &RefVisual, Option<&RenderLayers>, &Visibility, &InheritedVisibility, &ViewVisibility, Has<Mesh3d>, Option<&ChildOf>)>,
) {
    static REFS: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    let refs = REFS.get_or_init(|| {
        std::env::var("NVF_PORTAL_DEBUG_REFS").map(|v| v.split(',').filter_map(|h| u32::from_str_radix(h.trim().trim_start_matches("0x"), 16).ok()).collect()).unwrap_or_default()
    });
    if refs.is_empty() {
        return;
    }
    let cam = portal.open.as_ref().map(|o| o.camera);
    if cam != since.0 {
        *since = (cam, 0);
        return;
    }
    since.1 += 1;
    if cam.is_none() || since.1 != 60 {
        return;
    }
    for (e, r, l, v, iv, vv, mesh, parent) in &q {
        if refs.contains(&r.0) {
            info!("portal debug: {:08X} entity {e:?} mesh {mesh} layers {:?} visibility {:?} inherited {} view {} parent {:?}", r.0, l, v, iv.get(), vv.get(), parent.map(|p| p.parent()));
        }
    }
}

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
