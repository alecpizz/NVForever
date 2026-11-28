//! The roulette table of the roulette menu (`nvf_play::roulette`,
//! docs/spec-notes/roulette.md §6): the game's models drawn by a camera of their own into
//! a picture the menu shows behind its text ([`nvf_play::roulette::RouletteView`]).
//!
//! - The table is the casino's roulette model (CSNO MOD4, `NV_Roulette-Table<x>.NIF`): the
//!   felt, the wheel (`Roulette_Wheel`) in its housing, the ball (`Ball`, under the turning
//!   `Point01`) and a camera (`Camera01`, aimed at `Camera01.Target`); its `Spin_1`–`Spin_3`
//!   sequences (9.83 s each) turn the wheel, run the ball round and down into a pocket, and
//!   fly the camera from above the felt to the wheel [observed: the file's nodes and
//!   sequences]. Betting, the model stands in its stored pose; a spin plays one of the three
//!   (`nvf_play::roulette::Phase::Spinning`), held at its end while the result shows.
//! - The ball comes to rest at the same place on the wheel in a given sequence; the wheel is
//!   turned about its axis by the angle that brings the spun pocket under it
//!   [hypothesis: the original's way of matching the result is not known]. A pocket's angle
//!   on the wheel: the number strip (`nv_roulette03.dds`, `fnv_game::roulette::WHEEL_STRIP`)
//!   wraps the wheel's shape once, its left edge at −15.33° about the wheel's Z axis from
//!   its X axis, counter-clockwise as the picture runs, 38 equal cells [observed:
//!   `fnvtool nif decode` of the wheel's shape: texture U against vertex angle].
//! - The camera: the model's own, from `Camera01` towards `Camera01.Target` (a look-at
//!   sequence), its view 34.5° high [observed: the NiCamera's frustum, ±0.311 at a near
//!   plane of 1; hypothesis: the height is kept on wider screens].
//! - The bet points: the nodes of `nv_roulette-points.nif`, placed with the table's origin
//!   [hypothesis: checked by eye, the points fall on the felt's numbers]. The marker is
//!   `nv_roulette-bet.nif` at the point under it (`Bet_Selector`, or `Bet_Invalid` where the
//!   current bet cannot go [hypothesis: by the names]); each bet a stack of
//!   `nv_roulette-chip.nif`, as tall as the chips it takes in 100s, 25s, 10s, 5s and 1s, a
//!   tenth of the model's height a chip, at most the whole model [design].
//! - Lights: a key light from above the player's side and ambient light, on the render
//!   layer of its own [design: the file's three point lights (`Omni01`–`03`, riding with
//!   the camera) are not read].
//! - The pointer: a ray from the camera through the pointer meets the felt's plane; the
//!   nearest bet point within reach is the one under the pointer
//!   ([`nvf_play::roulette::RoulettePointer`]).

use crate::lockpick_view::{local_of, node_index, sampled};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::AmbientLight;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use fnv_game::roulette::{self as rules, WHEEL_STRIP};
use nvf_assets::anim_play::Clip;
use nvf_assets::model::{ModelCtx, PreparedModel, Rig};
use nvf_play::pause::AidTime;
use nvf_play::roulette::{Phase, RoulettePointer, RouletteScreen, RouletteView};
use std::collections::HashMap;
use std::sync::Arc;

pub const DEFAULT_TABLE: &str = "meshes\\terminals\\nv_roulette\\nv_roulette-table.nif";
pub const POINTS_MODEL: &str = "meshes\\terminals\\nv_roulette\\nv_roulette-points.nif";
pub const BET_MODEL: &str = "meshes\\terminals\\nv_roulette\\nv_roulette-bet.nif";
pub const CHIP_MODEL: &str = "meshes\\terminals\\nv_roulette\\nv_roulette-chip.nif";
/// The render layer of the table, its lights and its camera (lockpicking has 7, poker 8).
pub const ROULETTE_LAYER: usize = 10;
/// Where the table is put, far from the world and from the lock (Bevy space) [design].
const PLACE: Vec3 = Vec3::new(0.0, -200_000.0, 40_000.0);
/// The camera's view height, degrees: 2 atan(0.31066) [observed: the NiCamera's frustum].
const VFOV_DEG: f32 = 34.5;
/// Where the wheel's number strip starts, degrees about the wheel's Z axis (see above).
const STRIP_START_DEG: f32 = -15.33;
/// How near the pointer must come to a bet point, game units: the grid's (two across) and
/// the outside places' [design].
const REACH_GRID: f32 = 1.3;
const REACH_OUTSIDE: f32 = 4.5;
/// The chip model's height in chips [design].
const CHIPS_PER_MODEL: f32 = 10.0;
pub struct RouletteViewPlugin;

impl Plugin for RouletteViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TableCamera>()
            .add_systems(Update, show.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync))
            .add_systems(PostUpdate, world_camera.before(bevy::transform::TransformSystems::Propagate));
    }
}

/// Where the game's own camera looks from while a casino table's menu shows the table
/// where it stands in the world (Bevy space), and its view height (degrees); shared with
/// blackjack (`blackjack_view.rs`).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct TableCamera(pub Option<(Transform, f32)>);

/// The game's camera at the table's while [`TableCamera`] says so (after the player's
/// camera has been placed this frame, as the darts' throw line is); its field of view
/// put back after.
fn world_camera(want: Res<TableCamera>, mut cams: Query<(&mut Transform, &mut Projection), With<nvf_play::PlayerCamera>>, mut saved: Local<Option<f32>>) {
    let Ok((mut t, mut proj)) = cams.single_mut() else { return };
    let Projection::Perspective(p) = &mut *proj else { return };
    match want.0 {
        Some((at, fov_deg)) if at.translation.is_finite() && at.rotation.is_finite() => {
            *t = at;
            if saved.is_none() {
                *saved = Some(p.fov);
            }
            let fov = fov_deg.to_radians();
            if p.fov != fov {
                p.fov = fov;
            }
        }
        Some(_) => {}
        None => {
            if let Some(f) = saved.take() {
                p.fov = f;
            }
        }
    }
}

struct Table {
    rig: Rig,
    by_name: HashMap<String, Entity>,
    wheel: usize,
    camera: usize,
    target: usize,
    /// For each spin sequence (1–3): the ball's angle on the wheel at its end, degrees.
    ball_end: [Option<f32>; 3],
}

impl Table {
    fn clip(&self, n: u8) -> Option<&Clip> {
        self.rig.group(&format!("Spin_{n}"))
    }

    /// Every node in model space, posed by `clip` at `t`, the wheel turned by `turn`
    /// degrees about its own Z axis.
    fn space(&self, clip: Option<&Clip>, t: f32, turn: f32) -> Vec<Affine3A> {
        let mut out: Vec<Affine3A> = Vec::with_capacity(self.rig.nodes.len());
        for (i, n) in self.rig.nodes.iter().enumerate() {
            let local = if i == 0 { Affine3A::IDENTITY } else { sampled(&self.rig, clip, i, t).compute_affine() };
            let parent = n.parent.map_or(Affine3A::IDENTITY, |p| out[p]);
            let mut w = parent * local;
            if i == self.wheel && turn != 0.0 {
                w *= Affine3A::from_rotation_z(turn.to_radians());
            }
            out.push(w);
        }
        out
    }

    fn pose(&self, world: &[Affine3A], transforms: &mut Query<&mut Transform>) {
        for (i, n) in self.rig.nodes.iter().enumerate().skip(1) {
            let Some(e) = n.name.as_ref().and_then(|name| self.by_name.get(name)) else { continue };
            let parent = n.parent.map_or(Affine3A::IDENTITY, |p| world[p]);
            if let Ok(mut tr) = transforms.get_mut(*e) {
                *tr = local_of(world[i], parent);
            }
        }
    }
}

/// A clip's time `elapsed` seconds in, held at its end.
fn clip_time(c: Option<&Clip>, elapsed: f32) -> f32 {
    c.map_or(0.0, |c| (c.start + elapsed.max(0.0) * c.frequency).min(c.stop))
}

/// A pocket's angle on the wheel, degrees.
pub fn pocket_angle(pocket: u8) -> f32 {
    let k = WHEEL_STRIP.iter().position(|&p| p == pocket).unwrap_or(0) as f32;
    STRIP_START_DEG + 360.0 * (k + 0.5) / WHEEL_STRIP.len() as f32
}

/// The pocket at an angle on the wheel.
pub fn pocket_at(angle: f32) -> u8 {
    let n = WHEEL_STRIP.len() as f32;
    let k = ((angle - STRIP_START_DEG).rem_euclid(360.0) / 360.0 * n).floor() as usize;
    WHEEL_STRIP[k.min(WHEEL_STRIP.len() - 1)]
}

/// How far the chip model reaches under its origin (its height).
fn chip_depth(m: &PreparedModel) -> f32 {
    m.bounds.map_or(2.0, |(lo, hi)| (hi.z - lo.z).max(0.01))
}

/// The chips a bet's stack shows: the amount in 100s, 25s, 10s, 5s and 1s [design].
pub fn stack_chips(amount: u32) -> u32 {
    let mut left = amount;
    let mut n = 0;
    for d in [100, 25, 10, 5, 1] {
        n += left / d;
        left %= d;
    }
    n
}

#[derive(Default)]
struct Drawn {
    entities: Vec<Entity>,
    table: Option<Table>,
    /// Bet points by name, game space (model space of the table).
    points: HashMap<String, Vec3>,
    /// The parent of the chips and the marker (game space).
    space: Option<Entity>,
    /// The marker's two parts: the selector's shapes and the invalid sign's.
    marker: Option<Entity>,
    selector: Vec<Entity>,
    invalid: Vec<Entity>,
    chips: Vec<Entity>,
    chips_key: String,
    chip_model: Option<Arc<PreparedModel>>,
    camera: Option<Entity>,
    /// The camera in game space: eye and target, for the pointer's ray.
    eye: (Vec3, Vec3),
    cursor: Option<Vec2>,
    warned: bool,
    /// The table's place in the world (game space), when it stands where the world's does.
    world: Option<Affine3A>,
    /// The world's own drawing of that table, hidden while the menu's stands in for it:
    /// each entity and its visibility before.
    hidden: Vec<(Entity, Visibility)>,
}

impl Drawn {
    fn layer(&self) -> RenderLayers {
        RenderLayers::layer(if self.world.is_some() { 0 } else { ROULETTE_LAYER })
    }

    /// Model space of the table to Bevy space.
    fn to_bevy(&self, p: Vec3) -> Vec3 {
        match self.world {
            Some(a) => nvf_play::space::to_bevy(a.transform_point3(p).to_array()),
            None => PLACE + nvf_play::space::to_bevy(p.to_array()),
        }
    }
}

/// The menu's table model fitted onto the world's table (its model space into the
/// world's model space): the world's tables are other models (the casinos' clutter tables)
/// [observed: FalloutNV.esm ACTI `RouletteTableUltraLuxe`: `clutter\casino\
/// roulettetable01.nif`], so the menu's is turned by quarter turns until the long sides
/// agree, scaled to the world's length, centred on it and stood on its foot; of the two ways
/// round, the one that puts the menu's camera (`eye`, menu model space) on the side
/// nearer the player (`player`, world model space) [design].
pub fn fit((mlo, mhi): (Vec3, Vec3), (wlo, whi): (Vec3, Vec3), eye: Vec3, player: Vec3) -> Affine3A {
    let (me, we) = (mhi - mlo, whi - wlo);
    let (m_long_x, w_long_x) = (me.x >= me.y, we.x >= we.y);
    let k = (if w_long_x { we.x } else { we.y }) / (if m_long_x { me.x } else { me.y }).max(1e-3);
    let base = if m_long_x == w_long_x { 0.0 } else { std::f32::consts::FRAC_PI_2 };
    let (mc, wc) = ((mlo + mhi) * 0.5, (wlo + whi) * 0.5);
    let make = |turn: f32| {
        let r = Quat::from_rotation_z(turn);
        let c = r * (mc * k);
        let t = Vec3::new(wc.x - c.x, wc.y - c.y, wlo.z - mlo.z * k);
        Affine3A::from_scale_rotation_translation(Vec3::splat(k), r, t)
    };
    let (a, b) = (make(base), make(base + std::f32::consts::PI));
    if a.transform_point3(eye).distance(player) <= b.transform_point3(eye).distance(player) {
        a
    } else {
        b
    }
}

/// The lamp over a casino table while its menu is open in the world: the casinos' floors
/// are dim and the felt must be read; a warm spot light this high over the felt, about as
/// bright as a placed light of fade [`TABLE_LAMP`] [design].
pub const TABLE_LAMP: f32 = 1.2;
pub const LAMP_HEIGHT: f32 = 110.0;

/// The lamp over the felt at `felt` (Bevy space); despawned with the menu's table.
pub fn table_lamp(commands: &mut Commands, felt: Vec3) -> Entity {
    let light = SpotLight {
        color: Color::srgb(1.0, 0.93, 0.82),
        intensity: crate::lights::lumens(TABLE_LAMP, crate::sky::light_unit()),
        range: 3.0 * LAMP_HEIGHT,
        outer_angle: 0.75,
        inner_angle: 0.45,
        shadow_maps_enabled: false,
        ..default()
    };
    commands.spawn((Name::new("Casino table lamp"), light, Transform::from_translation(felt + Vec3::Y * LAMP_HEIGHT).looking_at(felt, Vec3::Z))).id()
}

/// The largest thing at a table hidden with it, game units across: stools and chairs, not
/// the floor or the walls [design].
pub const FURNITURE_MAX: f32 = 120.0;

/// What would stand in the menu camera's way at a world table (`nvf_play::roulette::
/// table_staff`): its actors, and the small models about it.
pub fn in_the_way(sim: &nvf_play::sim::SimState, play: &nvf_play::PlayWorld, models: &mut ModelCtx, table: fnv_sim::FormId) -> Vec<fnv_sim::FormId> {
    nvf_play::roulette::table_staff(sim, table)
        .into_iter()
        .filter(|r| {
            if sim.is_actor(r.0) {
                return true;
            }
            let Some(base) = sim.world.peek(*r).map(|s| s.base).or_else(|| sim.base_of(r.0)) else { return false };
            // Stools and chairs, loaded or not.
            if sim.forms.sig(base).is_some_and(|s| s == *b"FURN") {
                return true;
            }
            let Some(path) = nvf_play::dropped::model_path(&play.0, base, 0) else { return false };
            let full = format!("meshes\\{}", path.trim_start_matches("meshes\\").trim_start_matches("Meshes\\"));
            models.model(&full).and_then(|m| m.bounds).is_some_and(|(lo, hi)| (hi - lo).max_element() * sim.ref_scale(r.0) <= FURNITURE_MAX)
        })
        .collect()
}

/// Where a reference stands (game space), as the world draws it.
pub fn ref_place(sim: &nvf_play::sim::SimState, r: fnv_sim::FormId) -> Option<Affine3A> {
    let (pos, rot) = sim.world.peek(r).map(|s| (s.pos, s.rot)).or_else(|| sim.refs.get(r).map(|p| (p.pos, p.rot)))?;
    Some(nvf_play::moved::ref_affine(pos, rot, sim.ref_scale(r.0)))
}

/// Hide the world's own drawing of a reference; what each entity was, for [`unhide`].
pub fn hide_ref(r: u32, refs: &Query<(Entity, &nvf_play::RefVisual)>, visibility: &mut Query<&mut Visibility>) -> Vec<(Entity, Visibility)> {
    let mut out = Vec::new();
    for (e, v) in refs.iter() {
        if v.0 != r {
            continue;
        }
        if let Ok(mut vis) = visibility.get_mut(e) {
            out.push((e, *vis));
            *vis = Visibility::Hidden;
        }
    }
    out
}

pub fn unhide(hidden: &mut Vec<(Entity, Visibility)>, visibility: &mut Query<&mut Visibility>) {
    for (e, was) in hidden.drain(..) {
        if let Ok(mut vis) = visibility.get_mut(e) {
            *vis = was;
        }
    }
}

/// Spawn a model's shapes (no sequences) under `parent` at `at` (game space); each entity
/// with its shape's name and the names of the nodes above it.
fn spawn_shapes(commands: &mut Commands, m: &PreparedModel, parent: Entity, layer: &RenderLayers) -> Vec<(Entity, Vec<String>)> {
    m.shapes
        .iter()
        .map(|s| {
            let names: Vec<String> = s.name.iter().cloned().chain(s.ancestors.iter().cloned()).collect();
            let t = Transform::from_matrix(Mat4::from(m.root * s.transform));
            let e = commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), t, Visibility::Inherited, layer.clone(), ChildOf(parent))).id();
            (e, names)
        })
        .collect()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show(
    mut commands: Commands,
    screen: Res<RouletteScreen>,
    view: Option<Res<RouletteView>>,
    mut models: ModelCtx,
    window: Query<&Window, With<PrimaryWindow>>,
    time: AidTime,
    mut pointer: ResMut<RoulettePointer>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut drawn: Local<Drawn>,
    (sim, play, world_root, refs, mut table_camera): (Option<Res<nvf_play::sim::SimState>>, Option<Res<nvf_play::PlayWorld>>, Query<Entity, With<crate::world::WorldRoot>>, Query<(Entity, &nvf_play::RefVisual)>, ResMut<TableCamera>),
) {
    let Some(s) = screen.0.as_ref() else {
        if !drawn.entities.is_empty() || view.is_some() {
            for e in drawn.entities.drain(..) {
                commands.entity(e).despawn();
            }
            unhide(&mut drawn.hidden, &mut visibility);
            *drawn = Drawn::default();
            commands.remove_resource::<RouletteView>();
            pointer.point = None;
            table_camera.set_if_neq(TableCamera(None));
        }
        return;
    };
    if drawn.entities.is_empty() {
        let path = s.model.as_deref().map_or_else(|| DEFAULT_TABLE.to_string(), |m| format!("meshes\\{m}"));
        let loaded = (models.model(&path), models.model(POINTS_MODEL), models.model(BET_MODEL), models.model(CHIP_MODEL));
        let (Some(table), Some(points), Some(bet), Some(chip)) = loaded else {
            if !drawn.warned {
                warn!("roulette view: {path}, {POINTS_MODEL}, {BET_MODEL} or {CHIP_MODEL} not loaded; the menu is drawn without the table");
                drawn.warned = true;
            }
            return;
        };
        let (Some(rig), Some(point_rig)) = (table.rig.clone(), points.rig.clone()) else {
            if !drawn.warned {
                warn!("roulette view: the table's or the points' model has no sequences");
                drawn.warned = true;
            }
            return;
        };
        let names = ["Roulette_Wheel", "Ball", "Camera01", "Camera01.Target"].map(|n| node_index(&rig, n));
        let [Some(wheel), Some(ball), Some(camera_node), Some(target)] = names else {
            warn!("roulette view: {path} lacks the wheel, the ball or the camera");
            drawn.warned = true;
            return;
        };
        // At the table in the world: the menu's table stands where the world's does (the
        // world's hidden meanwhile), under the world root, drawn and lit with the world and
        // seen by the game's own camera; else alone, far off, by a camera of its own.
        let root = world_root.single().ok();
        drawn.world = s.table_ref.zip(sim.as_deref()).zip(play.as_deref()).filter(|_| root.is_some()).and_then(|((t, sim), play)| {
            let place = ref_place(sim, t)?;
            let base = sim.world.peek(t).map(|r| r.base).or_else(|| sim.base_of(t.0))?;
            let world_path = nvf_play::dropped::model_path(&play.0, base, 0)?;
            let world_model = models.model(&format!("meshes\\{}", world_path.trim_start_matches("meshes\\").trim_start_matches("Meshes\\")))?;
            let eye = crate::lockpick_view::model_space(&rig, None, 0.0).get(camera_node).map(|a| Vec3::from(a.translation))?;
            let player = sim.world.peek(fnv_sim::FormId::PLAYER_REF).map(|p| Vec3::from(p.pos))?;
            let f = fit(table.bounds?, world_model.bounds?, eye, place.inverse().transform_point3(player));
            info!("roulette view: the menu's table fitted onto {world_path} ({t}): scale {:.3}, turned {:.0}°", f.matrix3.x_axis.length(), f.matrix3.x_axis.y.atan2(f.matrix3.x_axis.x).to_degrees());
            Some(place * f)
        });
        let layer = drawn.layer();
        let (holder, to_bevy) = match (drawn.world, root) {
            (Some(at), Some(root)) => {
                let h = commands.spawn((Name::new("Roulette table (at the world's)"), Transform::from_matrix(Mat4::from(at)), Visibility::Inherited, ChildOf(root))).id();
                (h, Transform::IDENTITY)
            }
            _ => {
                let h = commands.spawn((Name::new("Roulette table"), Transform::from_translation(PLACE), Visibility::Inherited)).id();
                // Game space (Z up) into Bevy's (Y up), as the world root does.
                (h, Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)))
            }
        };
        let (_, table_drawn, by_name) = rig.spawn_nodes(&mut commands, holder, to_bevy, &path, 0.0);
        for e in &table_drawn {
            commands.entity(*e).insert(layer.clone());
        }
        let space = commands.spawn((to_bevy, Visibility::Inherited, ChildOf(holder))).id();
        // The marker.
        let marker = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(space))).id();
        let parts = spawn_shapes(&mut commands, &bet, marker, &layer);
        let is_invalid = |names: &[String]| names.iter().any(|n| n.to_ascii_lowercase().starts_with("bet_invalid"));
        let selector: Vec<Entity> = parts.iter().filter(|(_, n)| !is_invalid(n)).map(|(e, _)| *e).collect();
        let invalid: Vec<Entity> = parts.iter().filter(|(_, n)| is_invalid(n)).map(|(e, _)| *e).collect();
        // The bet points.
        let point_space = crate::lockpick_view::model_space(&point_rig, None, 0.0);
        let points: HashMap<String, Vec3> = point_rig
            .nodes
            .iter()
            .zip(&point_space)
            .filter_map(|(n, w)| {
                let name = n.name.as_ref()?.to_ascii_uppercase();
                rules::spot(&name).map(|_| (name, Vec3::from(w.translation)))
            })
            .collect();
        let mut own_camera = None;
        let mut lamp = None;
        if drawn.world.is_none() {
            let size = window.single().map(|w| Extent3d { width: w.physical_width().max(1), height: w.physical_height().max(1), depth_or_array_layers: 1 }).unwrap_or(Extent3d { width: 1280, height: 720, depth_or_array_layers: 1 });
            let mut image = Image::new_fill(size, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
            image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
            let image = models.images.add(image);
            let camera = commands
                .spawn((
                    Name::new("Roulette camera"),
                    Camera3d::default(),
                    Camera { order: -3, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
                    RenderTarget::Image(ImageRenderTarget::from(image.clone())),
                    Projection::Perspective(PerspectiveProjection { fov: VFOV_DEG.to_radians(), near: 1.0, far: 4000.0, ..default() }),
                    Tonemapping::Linear,
                    Msaa::Sample4,
                    AmbientLight { color: Color::WHITE, brightness: 350.0, ..default() },
                    Transform::from_translation(PLACE),
                    layer.clone(),
                ))
                .id();
            let to = |p: Vec3| PLACE + nvf_play::space::to_bevy(p.to_array());
            let key = commands
                .spawn((DirectionalLight { illuminance: 5000.0, ..default() }, Transform::from_translation(to(Vec3::new(10.0, -60.0, 90.0))).looking_at(to(Vec3::ZERO), Vec3::Y), layer.clone()))
                .id();
            own_camera = Some((camera, key, image));
        } else if let (Some(t), Some(a)) = (s.table_ref, drawn.world) {
            let staff = sim.as_deref().zip(play.as_deref()).map(|(sim, play)| in_the_way(sim, play, &mut models, t)).unwrap_or_default();
            info!("roulette view: {} things at the table hidden: {staff:?}", staff.len());
            for r in sim.as_deref().map_or_else(|| vec![t], |sim| nvf_play::roulette::table_twins(sim, play.as_deref(), t, "roulette")).into_iter().chain(staff) {
                let more = hide_ref(r.0, &refs, &mut visibility);
                drawn.hidden.extend(more);
            }
            let felt = points.values().fold(Vec3::ZERO, |sum, p| sum + *p) / points.len().max(1) as f32;
            let wheel_at = crate::lockpick_view::model_space(&rig, None, 0.0).get(wheel).map_or(felt, |w| Vec3::from(w.translation));
            // One over the felt, one over the wheel the spins' camera flies to.
            lamp = Some([felt, wheel_at].map(|p| table_lamp(&mut commands, nvf_play::space::to_bevy(a.transform_point3(p).to_array()))));
        }
        let mut t = Table { rig, by_name, wheel, camera: camera_node, target, ball_end: [None; 3] };
        for n in 1..=3u8 {
            let Some(c) = t.clip(n) else { continue };
            let end = t.space(Some(c), c.stop, 0.0);
            let local = end[wheel].inverse().transform_point3(end[ball].translation.into());
            t.ball_end[n as usize - 1] = Some(local.y.atan2(local.x).to_degrees());
        }
        info!(
            "roulette view: {path} drawn ({} shapes){}, {} bet points, chip model {} shapes (bounds {:?}), ball at the spins' ends: {}",
            table_drawn.len(),
            match (s.table_ref, drawn.world) {
                (Some(r), Some(a)) => format!(" at the world's table {r} ({:.0}, {:.0}, {:.0}; {} of its entities hidden)", a.translation.x, a.translation.y, a.translation.z, drawn.hidden.len()),
                _ => " alone (no table in the world)".to_string(),
            },
            points.len(),
            chip.shapes.len(),
            chip.bounds,
            t.ball_end.iter().map(|a| a.map_or("none".to_string(), |a| format!("{a:.1}° (pocket {})", rules::pocket_name(pocket_at(a))))).collect::<Vec<_>>().join(", ")
        );
        drawn.entities = vec![holder];
        drawn.entities.extend(lamp.into_iter().flatten());
        drawn.table = Some(t);
        drawn.points = points;
        drawn.space = Some(space);
        drawn.marker = Some(marker);
        drawn.selector = selector;
        drawn.invalid = invalid;
        drawn.chip_model = Some(chip);
        let image = own_camera.map(|(camera, key, image)| {
            drawn.entities.extend([camera, key]);
            drawn.camera = Some(camera);
            image
        });
        commands.insert_resource(RouletteView { image });
    }
    let layer = drawn.layer();
    let now = time.elapsed_secs();
    let Some(t) = drawn.table.as_ref() else { return };
    // The table's pose.
    let world = match s.phase {
        Phase::Betting => t.space(None, 0.0, 0.0),
        Phase::Spinning { since, clip, pocket } | Phase::Paid { since, clip, pocket, .. } => {
            let c = t.clip(clip);
            let turn = t.ball_end[clip as usize - 1].map_or(0.0, |b| b - pocket_angle(pocket));
            t.space(c, clip_time(c, now - since), turn)
        }
    };
    t.pose(&world, &mut transforms);
    // The camera: from Camera01 towards its target.
    let eye = Vec3::from(world[t.camera].translation);
    let aim = Vec3::from(world[t.target].translation);
    let looking = Transform::from_translation(drawn.to_bevy(eye)).looking_at(drawn.to_bevy(aim), Vec3::Y);
    if drawn.world.is_some() {
        table_camera.set_if_neq(TableCamera(Some((looking, VFOV_DEG))));
    } else if let Some(mut tr) = drawn.camera.and_then(|c| transforms.get_mut(c).ok()) {
        *tr = looking;
    }
    drawn.eye = (eye, aim);
    // The chips at the bet points.
    let key: String = s.table.bets.iter().map(|(b, a)| format!("{}={a};", b.point)).collect();
    if key != drawn.chips_key {
        for e in drawn.chips.drain(..) {
            commands.entity(e).despawn();
        }
        if let (Some(space), Some(m)) = (drawn.space, drawn.chip_model.clone()) {
            let stacks: Vec<(Vec3, u32)> = s.table.bets.iter().filter_map(|(b, a)| drawn.points.get(&b.point).map(|p| (*p, *a))).collect();
            for (p, amount) in stacks {
                let height = (stack_chips(amount) as f32 / CHIPS_PER_MODEL).clamp(1.0 / CHIPS_PER_MODEL, 1.0);
                // The model hangs under its origin [observed: its bounds, z −2.04 to 0]: the
                // stack is lifted so its foot stands on the felt.
                let lift = chip_depth(&m) * height;
                let stack = commands.spawn((Transform::from_translation(p + Vec3::Z * lift).with_scale(Vec3::new(1.0, 1.0, height)), Visibility::Inherited, ChildOf(space))).id();
                spawn_shapes(&mut commands, &m, stack, &layer);
                drawn.chips.push(stack);
            }
        }
        drawn.chips_key = key;
    }
    // The marker over the bet point under it, above any chips there.
    let betting = s.phase == Phase::Betting;
    if let Some(marker) = drawn.marker {
        let at = drawn.points.get(&s.cursor).copied();
        if let (Some(p), Ok(mut tr)) = (at, transforms.get_mut(marker)) {
            let on = s.table.on(&s.cursor);
            let depth = drawn.chip_model.as_deref().map_or(2.0, chip_depth);
            let lift = if on > 0 { depth * (stack_chips(on) as f32 / CHIPS_PER_MODEL).clamp(1.0 / CHIPS_PER_MODEL, 1.0) } else { 0.0 };
            tr.translation = p + Vec3::Z * lift;
        }
        if let Ok(mut v) = visibility.get_mut(marker) {
            v.set_if_neq(if betting && at.is_some() && s.help.is_none() { Visibility::Inherited } else { Visibility::Hidden });
        }
        let valid = s.can_place();
        for (list, show) in [(&drawn.selector, valid), (&drawn.invalid, !valid)] {
            for e in list {
                if let Ok(mut v) = visibility.get_mut(*e) {
                    v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
                }
            }
        }
    }
    // The pointer over the felt.
    let Ok(w) = window.single() else { return };
    let cursor = w.cursor_position();
    if cursor == drawn.cursor || !betting || s.help.is_some() {
        return;
    }
    drawn.cursor = cursor;
    let Some(c) = cursor else { return };
    let size = Vec2::new(w.width(), w.height());
    let felt_z = drawn.points.values().next().map_or(0.0, |p| p.z);
    let found = pick(drawn.eye, c, size, felt_z, &drawn.points);
    if found != pointer.point {
        pointer.point = found;
        pointer.moves += 1;
    }
}

/// The bet point under the pointer at `c` (logical pixels in a window of `size`), the
/// camera at `eye` looking at `aim` (game space).
fn pick((eye, aim): (Vec3, Vec3), c: Vec2, size: Vec2, felt_z: f32, points: &HashMap<String, Vec3>) -> Option<String> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let f = (aim - eye).normalize_or_zero();
    let r = f.cross(Vec3::Z).normalize_or_zero();
    let up = r.cross(f);
    let tan = (VFOV_DEG.to_radians() / 2.0).tan();
    let ndc = Vec2::new(c.x / size.x * 2.0 - 1.0, 1.0 - c.y / size.y * 2.0);
    let dir = f + r * ndc.x * tan * size.x / size.y + up * ndc.y * tan;
    if dir.z.abs() < 1e-6 {
        return None;
    }
    let k = (felt_z - eye.z) / dir.z;
    if k <= 0.0 {
        return None;
    }
    let hit = eye + dir * k;
    points
        .iter()
        .map(|(n, p)| (n, Vec2::new(p.x - hit.x, p.y - hit.y).length()))
        .filter(|(n, d)| *d <= if n.starts_with('S') { REACH_OUTSIDE } else { REACH_GRID })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(n, _)| n.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pockets_and_angles_agree() {
        for p in 0..rules::POCKETS as u8 {
            assert_eq!(pocket_at(pocket_angle(p)), p);
        }
        // The strip's first cell, 35, runs from −15.33° on.
        assert_eq!(pocket_at(-15.0), 35);
        assert_eq!(pocket_at(-16.0), 14);
    }

    #[test]
    fn the_menu_table_is_fitted_onto_the_worlds() {
        // A menu table 20 long in x, the world's 200 long in y, its foot at z 0; the
        // player on the world's -x side: the menu's camera (at its -y side) ends there.
        let m = (Vec3::new(-10.0, -5.0, -1.0), Vec3::new(10.0, 5.0, 2.0));
        let w = (Vec3::new(-40.0, -100.0, 0.0), Vec3::new(40.0, 100.0, 30.0));
        let f = fit(m, w, Vec3::new(0.0, -8.0, 6.0), Vec3::new(-300.0, 0.0, 0.0));
        assert!((f.matrix3.x_axis.length() - 10.0).abs() < 1e-3, "scaled to the world's length");
        let (lo, hi) = (f.transform_point3(m.0), f.transform_point3(m.1));
        assert!((lo.y.min(hi.y) + 100.0).abs() < 1e-2 && (lo.y.max(hi.y) - 100.0).abs() < 1e-2, "long sides agree: {lo} {hi}");
        assert!(lo.z.abs() < 1e-3, "on its foot");
        assert!(f.transform_point3(Vec3::new(0.0, -8.0, 6.0)).x < 0.0, "the camera on the player's side");
    }

    #[test]
    fn stacks_count_chips_by_denomination() {
        assert_eq!(stack_chips(1), 1);
        assert_eq!(stack_chips(35), 2);
        assert_eq!(stack_chips(200), 2);
        assert_eq!(stack_chips(49), 7);
    }

    #[test]
    fn the_pointer_finds_the_point_it_looks_at() {
        let points: HashMap<String, Vec3> = [("D12".to_string(), Vec3::new(0.0, 5.33, 0.0)), ("S08".to_string(), Vec3::new(-4.0, -10.67, 0.0))].into_iter().collect();
        // Looking straight down at D12 from above, the pointer in the middle.
        let eye = (Vec3::new(0.0, 5.0, 60.0), Vec3::new(0.0, 5.33, 0.0));
        assert_eq!(pick(eye, Vec2::new(640.0, 360.0), Vec2::new(1280.0, 720.0), 0.0, &points).as_deref(), Some("D12"));
        assert_eq!(pick(eye, Vec2::new(0.0, 0.0), Vec2::new(1280.0, 720.0), 0.0, &points), None);
    }
}
