//! Particle effects of add-on nodes (docs/spec-notes/particles.md): the sparks and smoke
//! of a muzzle flash, a missile's trail, the flamer's flame, the dust of an impact.
//!
//! - **Add-on nodes.** A model's `BSValueNode` named `AddOnNode<n>` stands for the ADDN
//!   record with that node index, whose model (`meshes\mps\…`) holds one or more
//!   `NiParticleSystem`s under a `BSMasterParticleSystem` [documented: xEdit ADDN;
//!   nif.xml]. Each add-on node in the world is an **emitter** of those systems for as
//!   long as it exists ([`AddonEmitter`]); the particles belong to the system, live in
//!   the world and outlast the node [hypothesis: the meaning of "master particle
//!   system", from the systems' `world_space` flag and from their particle counts,
//!   which equal birth rate × longest life × the emitter controller's `max_emitters`].
//! - **A system** is read by `nvf_assets::particles`; its particles are simulated here:
//!   born at the emitter's rate in its volume, thrown within its cone about the emitter
//!   object's +z, then aged, pulled by its gravity modifiers (in the world's axes),
//!   slowed by its drag modifiers, turned, sized by grow and fade, coloured over their
//!   life. A system holds at most its data block's vertex count of particles, and
//!   serves at most `max_emitters` emitters a frame.
//! - **Drawn** as one mesh per system, a square facing the camera for each particle,
//!   with the system's own texture, blending and vertex colours.
//! - **Model emitters** ([`ModelEmitter`]): a model's own particle systems, emitted from
//!   where its emitter objects are in the file (explosions, `shot_fx`).
//! - **Modifiers switched on and off** (`NiPSysModifierActiveCtlr`): a gravity or drag
//!   modifier acts on a particle while the emitter it came from is at a time its keys
//!   say on (`nvf_assets::particles::ModifierActive`).
//! - **Mesh emitters** start particles on their meshes (vertices, face or edge centres,
//!   anywhere on the faces or edges) thrown along the mesh's normal there; **strip
//!   systems** trail a strip of recorded points behind each particle, drawn as a ribbon;
//!   the **bomb** modifier pushes particles away from its object, **planar colliders**
//!   bounce or kill them; float controllers key emitter fields, gravity strength and
//!   birth rates over the emitter's time (particles.md §6). Bombs, colliders and
//!   spherical gravity sit where the emitter's node placed them when the particle was
//!   born (`Particle::frame`).
//! - **Placed models** emit their own systems and their add-on nodes' from a holder at
//!   the reference (`world::spawn_emitters`: Gomorrah's roof fires), not while hidden;
//!   a placed model's own live particles go at once when it is hidden or gone (a taken
//!   snow globe, #1596; log `particles: <ref> is not drawn any more: its <n> live
//!   particles go`);
//!   a placed model's own systems are that reference's own, and a master system serves
//!   the add-on nodes nearest the camera first (particles.md §7). References a script
//!   enables (`nvf_play::script_draw`) emit too.
//! - **Damage stages.** An add-on node below a `BSDamageStage` node emits only while its
//!   reference is drawn at one of that node's model damage stages ([`StagedAddon`],
//!   docs/spec-notes/destruction-stages.md): a motorcycle's `DamageStage1-3` smoke starts
//!   when a hit takes it to stage 1, never on an intact or indestructible (STAT) bike.
//!   Log: `damage stage: <ref> add-on node <n> waits for model stages <lo>-<hi> (at <s>)`
//!   when one is placed, then `… on at model stage <s>` / `… off at model stage <s>`.
//! - **Sequences of drawn models** (particles.md §6.7): a placed model drawn with its own
//!   nodes (a [`crate::object_anims::GroupRig`]) whose sequences drive a system's emitter
//!   emits by the sequence playing on it, on that sequence's time, and not at all while it
//!   plays none of them; and from its emitter object's node as the sequence moves it (the
//!   REPCONN rockets' exhaust rides with the rockets). Modifiers switched on and off by
//!   the sequences follow the one a particle was born under.
//! - **Built only where a camera may see it** (issue #1417): emission, simulation, random
//!   draws, lifetimes and clean-up run every frame for every system as before; only the
//!   geometry build and the mesh write are left out for a system whose live particles'
//!   box ([`particle_bounds`]) lies outside every active 3D camera that draws its layers
//!   (the main camera, the water reflection camera while it is on, a scope's lens camera;
//!   [`FrameViews`]). Its render entity is hidden meanwhile (its emitter is not touched),
//!   so its old mesh cannot draw. When in doubt it is built and shown: no box (no finite
//!   particle), an entity without a placement, no active camera, a camera drawing its
//!   layers whose frustum is not set. The test uses each frustum's four sides only, not its near or far plane:
//!   a box between a camera and its near plane counts as seen, and the reflection
//!   camera's near plane (the water's oblique clip plane) culls nothing [design].
//! - **When** (the first frame a system comes into view is drawn with fresh geometry):
//!   the `Update` chain ends with [`take_basis`], which takes the main camera's right, up
//!   and backward where the draw took them before, so a visible system's geometry is bit
//!   for bit what it was. [`draw`] runs in `PostUpdate` in [`ParticleDrawSet`]: after the
//!   transforms are propagated and the frusta updated (`VisibilitySystems::UpdateFrusta`,
//!   which `reflection::update_reflection` precedes, switching its camera on or off and
//!   placing it), before visibility propagation (so hiding or showing takes effect this
//!   frame) and before `AssetEventSystems` (so a rebuilt mesh reaches the renderer this
//!   frame, as when the draw ran in `Update`). The test thus sees the cameras this frame
//!   is rendered with, and a system coming into view is rebuilt, shown and uploaded in
//!   that same frame; the visibility of the frame before is never used. Hiding or
//!   showing a system takes its blended draw out of or back into the renderer's sorted
//!   list, so overlapping blended surfaces at exactly the same sort distance may draw in
//!   another order, as with any visibility change; the vertices and their order are
//!   unchanged [design: Bevy's sorted phases sort by distance only].
//! - **The world camera** is found by its marker (`FlyCam`, main.rs `setup`), not as the
//!   only 3D camera other than the reflection's. Before, the draw stopped (every system
//!   froze where it was, shown or hidden) whenever another 3D camera existed: a 2D scope's,
//!   a 3D scope's lens camera, a menu view's. Particles now keep animating, and keep being
//!   culled and shown again, while a scope is up [design: an intended change, issue
//!   #1417 review]. Without the world camera (none spawned) nothing is built, shown or
//!   hidden.
//! - Not built: turbulence, spawn-on-death; a mesh emitter's meshes stay where the file
//!   puts them; gravity strength keys from the first sequence only.

use bevy::asset::AssetEventSystems;
use bevy::camera::primitives::{Aabb, Frustum};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers, VisibilitySystems};
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, MeshVertexAttribute, VertexAttributeValues};
use bevy::prelude::*;
use fnv_combat::rng::SplitMix;
use nvf_assets::anim_play::Playing;
use nvf_assets::model::ModelCtx;
use nvf_assets::particles::{particle_mesh, InitialVelocity, ParticleSystemDef, PreparedParticles};
use nvf_play::script::ScriptState;
use nvf_play::script_draw::{ScriptDrawnModel, ScriptDrawnRig, ScriptDrawnRoot};
use nvf_play::sfx::Sfx;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::world::WorldRoot;

/// The steps a second a drag modifier's percentage is taken per [hypothesis: fitted so
/// the flamer's flame, thrown at 2,700 units a second, ends near its projectile's range
/// of 640 units; docs/spec-notes/particles.md].
pub const DRAG_STEPS_PER_SECOND: f32 = 30.0;

/// An add-on node in the world: it emits the particles of ADDN `index` while it exists.
#[derive(Component)]
pub struct AddonEmitter {
    pub index: i32,
    age: f32,
    /// The placed reference it belongs to (the exit count of placed emitters).
    placed: Option<u32>,
}

impl AddonEmitter {
    pub fn new(index: i32) -> AddonEmitter {
        AddonEmitter { index, age: 0.0, placed: None }
    }

    /// An add-on node of placed reference `r` (a barrel's fire, a vehicle's smoke).
    pub fn placed(index: i32, r: u32) -> AddonEmitter {
        AddonEmitter { index, age: 0.0, placed: Some(r) }
    }
}

/// An add-on node below a model's `BSDamageStage` node, on placed reference `r`: an
/// [`AddonEmitter`] of ADDN `index` while the reference's model damage stage
/// (`Game::model_damage_stage`) is within `stages` [hypothesis: the node's children are
/// the model's look at those stages only, docs/spec-notes/destruction-stages.md].
#[derive(Component)]
pub struct StagedAddon {
    pub r: u32,
    pub index: i32,
    pub stages: (i32, i32),
    /// Last stage seen; `None` before the first look.
    at: Option<i32>,
}

/// Make node `i` of `nodes` (entity `e`) the emitter of ADDN `index` for placed
/// reference `r`: at once, or as a [`StagedAddon`] when it is below a `BSDamageStage`.
pub fn place_addon(commands: &mut Commands, e: Entity, r: u32, nodes: &[nvf_assets::model::ModelNode], i: usize, index: i32) {
    match nvf_assets::model::damage_stages_of(nodes, i) {
        Some(stages) => commands.entity(e).insert(StagedAddon { r, index, stages, at: None }),
        None => commands.entity(e).insert(AddonEmitter::placed(index, r)),
    };
}

/// Switch staged add-on nodes on and off as their references' damage stages change.
fn staged_addons(mut commands: Commands, script: Option<Res<ScriptState>>, mut staged: Query<(Entity, &mut StagedAddon)>) {
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    for (e, mut s) in &mut staged {
        let stage = game.map_or(0, |g| g.model_damage_stage(fnv_sim::FormId(s.r)) as i32);
        if s.at == Some(stage) {
            continue;
        }
        let (lo, hi) = s.stages;
        let shown = (lo..=hi).contains(&stage);
        let was = s.at.is_some_and(|a| (lo..=hi).contains(&a));
        if s.at.is_none() && !shown {
            info!("damage stage: {:08X} add-on node {} waits for model stages {lo}-{hi} (at {stage})", s.r, s.index);
        }
        if shown && !was {
            commands.entity(e).insert(AddonEmitter::placed(s.index, s.r));
            info!("damage stage: {:08X} add-on node {} on at model stage {stage}", s.r, s.index);
        } else if !shown && was {
            commands.entity(e).remove::<AddonEmitter>();
            info!("damage stage: {:08X} add-on node {} off at model stage {stage}", s.r, s.index);
        }
        s.at = Some(stage);
    }
}

/// A model in the world that emits the particle systems inside its own file (an
/// explosion's flash, smoke and shrapnel) while it exists, each from its emitter object's
/// place in the file under this entity [hypothesis: as an add-on node emits its ADDN's;
/// the systems' rate and on/off keys come from the model's sequence when its emitter
/// controllers leave them to it, nvf_assets::particles].
#[derive(Component)]
pub struct ModelEmitter {
    pub path: String,
    age: f32,
    /// The placed reference whose model it is: its systems are its own, not shared with
    /// the other references of the same model (particles.md §7.1).
    placed: Option<u32>,
    /// The sequence its drawn model played last frame and how far into it (seconds of
    /// the sequence's time): births between that and now (particles.md §6.7).
    clock: Option<(String, f32)>,
    /// A held weapon's model ([`ModelEmitter::held`]): its systems emit only as the
    /// holder's clips say, by system name: on, and the clip's birth rate where it keys
    /// one. A system not named does not emit.
    pub driven: Option<HashMap<String, Option<f32>>>,
    /// A factor on the size of the particles it gives birth to from now on (1: as the
    /// file says). Placed reference scale leaves the size alone (particles.md §7.7).
    pub size: f32,
    /// Grass-fire flame holders contribute their scale to the radius factor [design].
    pub(crate) scale_radius_with_holder: bool,
    /// Owned by a script-attached model (`AttachModel` / camera overlay): kept isolated
    /// from the host reference's placed particle systems (`instance_refs`) [design].
    attached: bool,
    /// Optional render layer for spawned particle meshes (e.g. camera overlay layers).
    layer: Option<RenderLayers>,
    /// Optional game-space root entity (e.g. a camera overlay holder) instead of
    /// `WorldRoot`: particles simulate and draw in this root's local game space [design].
    space_root: Option<Entity>,
}

impl ModelEmitter {
    pub fn new(path: impl Into<String>) -> ModelEmitter {
        ModelEmitter { path: path.into(), age: 0.0, placed: None, clock: None, driven: None, size: 1.0, scale_radius_with_holder: false, attached: false, layer: None, space_root: None }
    }

    /// The model of placed reference `r` (a dust cloud, a sconce's flame, a smoke column).
    pub fn placed(path: impl Into<String>, r: u32) -> ModelEmitter {
        ModelEmitter { path: path.into(), age: 0.0, placed: Some(r), clock: None, driven: None, size: 1.0, scale_radius_with_holder: false, attached: false, layer: None, space_root: None }
    }

    /// The model of placed reference `r` whose drawn rig starts its Idle `start` seconds
    /// in ("Random Anim Start" bases): the emitter's own time starts there too. Systems
    /// the rig's playing sequence drives follow that sequence's clock (§6.7); this keeps
    /// the rest, and the mover of those that are not driven (particles.md §6.8), in step
    /// with the rig and its sounds.
    pub fn placed_from(path: impl Into<String>, r: u32, start: f32) -> ModelEmitter {
        ModelEmitter { path: path.into(), age: start.max(0.0), placed: Some(r), clock: None, driven: None, size: 1.0, scale_radius_with_holder: false, attached: false, layer: None, space_root: None }
    }

    /// The model of a weapon in an actor's hand: its systems stay off until the actor's
    /// clips switch them on (the dynamite's fuse sparks while the throw clip lights it,
    /// particles.md §7.6); `driven` is set each frame by whoever plays the clips.
    pub fn held(path: impl Into<String>) -> ModelEmitter {
        ModelEmitter { path: path.into(), age: 0.0, placed: None, clock: None, driven: Some(HashMap::new()), size: 1.0, scale_radius_with_holder: false, attached: false, layer: None, space_root: None }
    }

    /// An attached NIF model's own particle emitter (`AttachModel` or a camera overlay):
    /// isolated by its emitter entity instance so it never collides with the host
    /// reference's placed particle systems, optionally drawn on `layer` and simulated
    /// under `space_root` instead of `WorldRoot` [design].
    pub fn attached(path: impl Into<String>, r: u32, layer: Option<RenderLayers>, space_root: Option<Entity>) -> ModelEmitter {
        ModelEmitter { path: path.into(), age: 0.0, placed: Some(r), clock: None, driven: None, size: 1.0, scale_radius_with_holder: false, attached: true, layer, space_root }
    }
}

/// [design, #2685] An ANIO hierarchy driven by an actor idle's external KF.
#[derive(Component)]
pub struct IdleParticleRig {
    pub nodes: HashMap<String, Entity>,
    pub clip: Arc<nvf_assets::anim_play::Clip>,
}

/// [design, #2685] Particle meshes live in world space, but their materials use
/// the attachment's idle clock and the ordinary rig material player.
#[derive(Component)]
pub struct IdleParticleMaterial(Entity);

pub fn sync_idle_materials(
    owners: Query<&Playing, Without<IdleParticleMaterial>>,
    mut particles: Query<(&IdleParticleMaterial, &mut Playing)>,
) {
    for (owner, mut player) in &mut particles {
        if let Ok(source) = owners.get(owner.0) {
            player.time = source.time;
        }
    }
}

/// A model emitter drawn with its own nodes: the rig's entity, and the sequence playing
/// on it with the span of its time this frame covers (`None`: none plays).
struct Drive {
    rig: Entity,
    playing: Option<(String, f32, f32)>,
    /// The sequence began this frame (logged).
    began: bool,
}

/// Whose systems: an ADDN's (by node index) or a model file's own (by path).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Source {
    Addon(i32),
    Model(String),
}

impl Source {
    fn label(&self) -> String {
        match self {
            Source::Addon(i) => format!("add-on node {i}"),
            Source::Model(path) => path.clone(),
        }
    }
}

/// Live systems: a source's shared ones (`instance` `None`: an ADDN's master systems, a
/// shot effect model's), or one placed model's own (`instance`: its emitter entity).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    source: Source,
    instance: Option<Entity>,
}

struct Particle {
    /// Its emitter node's place in the world when it was born: where its file's bombs,
    /// colliders and gravity points are.
    frame: Affine3A,
    pos: Vec3,
    vel: Vec3,
    /// The scale of its speed and gravity when it was born: the reference's
    /// ([`frame_scale`]) times the system node's (particles.md §7.7).
    scale: f32,
    /// Its emitter's time when it was born: the emitter's time now is this plus `age`
    /// (for modifiers switched on and off on the emitter's time).
    born_at: f32,
    /// The sequence (index among the file's) whose time `born_at` is, when a drawn
    /// model's sequence drove its emitter (particles.md §6.7).
    sequence: Option<usize>,
    age: f32,
    life: f32,
    radius: f32,
    angle: f32,
    spin: f32,
    tex: usize,
    /// A strip system's recorded points, oldest first, and the time since the last.
    trail: Vec<Vec3>,
    trail_clock: f32,
}

struct Live {
    def: ParticleSystemDef,
    particles: Vec<Particle>,
    /// The previous frame's mesh storage. Geometry is built here, then exchanged with
    /// the mesh's attribute vectors so steady-state draws retain their capacities.
    geometry: ParticleGeometry,
    mesh: Handle<Mesh>,
    entity: Entity,
    /// Emitters served this frame.
    served: usize,
    /// The part of a particle owed from earlier frames. Kept by the system, not by the
    /// emitter: a muzzle flash lives one frame, in which its share of the birth rate is
    /// about one particle, and a count kept by each flash would round most of them away
    /// [design].
    owed: f32,
    /// Simulated under a camera-overlay holder (`space_root`): billboards face the local
    /// camera basis (`[Vec3::X, Vec3::Z, -Vec3::Y]`) instead of the world camera's [design].
    camera_local: bool,
}

/// CPU-side particle geometry exchanged with a live mesh after each draw.
///
/// The mesh and this scratch storage each retain one set of capacities. Replacing mesh
/// attributes with new vectors every frame would drop the mesh's old allocation; swapping
/// lets both sets reach their high-water marks and be reused on subsequent frames.
///
/// The ADDN and shot effect systems live as long as the run, so a burst would otherwise
/// pin its high-water mark for good: each rebuilt set gives back memory once it uses
/// under a quarter of it ([`trim`]), so both sets stay within about four times the
/// last frames' geometry, and a system that empties drops its scratch set.
#[derive(Default)]
struct ParticleGeometry {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    /// A ribbon's points (its trail and its place), reused from ribbon to ribbon.
    points: Vec<Vec3>,
}

/// Shrink `v` to twice its length when it uses under a quarter of its capacity.
fn trim<T>(v: &mut Vec<T>) {
    if v.len() < v.capacity() / 4 {
        v.shrink_to(v.len() * 2);
    }
}

impl ParticleGeometry {
    fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.uvs.clear();
        self.colors.clear();
        self.indices.clear();
    }

    fn trim(&mut self) {
        trim(&mut self.positions);
        trim(&mut self.normals);
        trim(&mut self.uvs);
        trim(&mut self.colors);
        trim(&mut self.indices);
    }

    fn rebuild(&mut self, d: &ParticleSystemDef, particles: &[Particle], right: Vec3, up: Vec3, back: Vec3) {
        self.clear();
        let n = particles.len();
        self.positions.reserve(n * 4);
        self.uvs.reserve(n * 4);
        self.colors.reserve(n * 4);
        self.indices.reserve(n * 6);
        if d.strip.is_some() {
            // Ribbons through each particle's recorded points and its place, facing the
            // camera; the texture rectangle's width across, its height along, the oldest
            // point at its bottom (particles.md §6.2).
            for q in particles {
                let points = &mut self.points;
                points.clear();
                points.extend(q.trail.iter().copied().chain([q.pos]));
                let count = points.len();
                if count < 2 {
                    continue;
                }
                let t = (q.age / q.life).clamp(0.0, 1.0);
                let half = q.radius * d.scale(q.age, q.life);
                let [u, w, v, h] = d.subtextures.get(q.tex).copied().unwrap_or([0.0, 1.0, 0.0, 1.0]);
                let tint = d.tint(t);
                let e = d.emitter.color;
                let colour = display_colour([tint[0] * e[0], tint[1] * e[1], tint[2] * e[2], tint[3] * e[3] * keyed_alpha(d, q)]);
                let base = self.positions.len() as u32;
                for (i, p) in self.points.iter().enumerate() {
                    let tangent = self.points[(i + 1).min(count - 1)] - self.points[i.saturating_sub(1)];
                    let side = tangent.cross(back).normalize_or(right) * half;
                    self.positions.extend([(*p - side).to_array(), (*p + side).to_array()]);
                    let along = v + h * (1.0 - i as f32 / (count - 1) as f32);
                    self.uvs.extend([[u, along], [u + w, along]]);
                    self.colors.extend([colour; 2]);
                }
                for i in 0..(count as u32 - 1) {
                    let a = base + i * 2;
                    self.indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
                }
            }
        }
        for (k, q) in particles.iter().enumerate().filter(|_| d.strip.is_none()) {
            let t = (q.age / q.life).clamp(0.0, 1.0);
            let half = q.radius * d.scale(q.age, q.life);
            let (s, c) = q.angle.sin_cos();
            let (x, y) = ((right * c + up * s) * half, (up * c - right * s) * half);
            for corner in [-x - y, x - y, x + y, -x + y] {
                self.positions.push((q.pos + corner).to_array());
            }
            let [u, w, v, h] = d.subtextures.get(q.tex).copied().unwrap_or([0.0, 1.0, 0.0, 1.0]);
            self.uvs.extend([[u, v + h], [u + w, v + h], [u + w, v], [u, v]]);
            let tint = d.tint(t);
            let e = d.emitter.color;
            self.colors.extend([display_colour([tint[0] * e[0], tint[1] * e[1], tint[2] * e[2], tint[3] * e[3] * keyed_alpha(d, q)]); 4]);
            let b = (k * 4) as u32;
            self.indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        self.normals.resize(self.positions.len(), back.to_array());
        self.trim();
        trim(&mut self.points);
    }

    /// Replace every live attribute's contents while retaining both the mesh and scratch
    /// capacities. [`particle_mesh`] establishes these exact attribute and index types;
    /// should a mesh lack one or hold another type, the geometry is inserted instead
    /// (that set's capacity goes to the mesh) and false is returned.
    fn swap_into_mesh(&mut self, mesh: &mut Mesh) -> bool {
        let mut matched = swap_attribute(mesh, Mesh::ATTRIBUTE_POSITION, &mut self.positions, |v| match v {
            VertexAttributeValues::Float32x3(v) => Some(v),
            _ => None,
        });
        matched &= swap_attribute(mesh, Mesh::ATTRIBUTE_NORMAL, &mut self.normals, |v| match v {
            VertexAttributeValues::Float32x3(v) => Some(v),
            _ => None,
        });
        matched &= swap_attribute(mesh, Mesh::ATTRIBUTE_UV_0, &mut self.uvs, |v| match v {
            VertexAttributeValues::Float32x2(v) => Some(v),
            _ => None,
        });
        matched &= swap_attribute(mesh, Mesh::ATTRIBUTE_COLOR, &mut self.colors, |v| match v {
            VertexAttributeValues::Float32x4(v) => Some(v),
            _ => None,
        });
        if let Some(Indices::U32(values)) = mesh.indices_mut() {
            std::mem::swap(values, &mut self.indices);
        } else {
            mesh.insert_indices(Indices::U32(std::mem::take(&mut self.indices)));
            matched = false;
        }
        matched
    }
}

/// Swap `scratch` with the mesh's `attribute` when `values` finds a vector of its type
/// there; otherwise insert it (taking its storage) and return false.
fn swap_attribute<T>(mesh: &mut Mesh, attribute: MeshVertexAttribute, scratch: &mut Vec<T>, values: fn(&mut VertexAttributeValues) -> Option<&mut Vec<T>>) -> bool
where
    Vec<T>: Into<VertexAttributeValues>,
{
    if let Some(v) = mesh.attribute_mut(attribute.id).and_then(values) {
        std::mem::swap(v, scratch);
        return true;
    }
    mesh.insert_attribute(attribute, std::mem::take(scratch));
    false
}

#[derive(Resource)]
pub struct Particles {
    /// The live systems of each ADDN index, shot effect model or placed model instance;
    /// `None`: none we can read.
    systems: HashMap<Key, Option<Vec<Live>>>,
    reported: HashSet<Source>,
    /// Sources whose systems were logged (once each, however many instances).
    described: HashSet<Source>,
    /// Particles born, per source and system (by name, in file order): the exit report.
    tally: HashMap<Source, Vec<(String, u64)>>,
    /// Placed references with an emitter, and those whose emitters gave birth.
    placed_seen: HashSet<u32>,
    placed_emitted: HashSet<u32>,
    /// The placed reference of each placed model's emitter, for the log.
    instance_refs: HashMap<Entity, u32>,
    /// The host reference of each attached/overlay model's emitter (kept separate from
    /// `instance_refs` so attached emitters stay isolated from host placed systems).
    attached_refs: HashMap<Entity, u32>,
    /// Most placed model instances live at once.
    most_instances: usize,
    rng: SplitMix,
    pub born: u64,
    pub most_alive: usize,
    /// The main camera's right, up and backward in game space taken this frame by
    /// [`take_basis`] for [`draw`] (module notes, "When"); `None`: nothing is drawn.
    basis: Option<[Vec3; 3]>,
}

impl Particles {
    /// The live systems' meshes (the profile aid's `profile change sources:` line).
    pub fn mesh_ids(&self) -> impl Iterator<Item = AssetId<Mesh>> + '_ {
        self.systems.values().flatten().flatten().map(|l| l.mesh.id())
    }

    /// The systems held (placed models' and shared ones), for the stress census. A
    /// system's entity always holds its materials, so an entry counts as idle when none of
    /// its systems has a particle alive (a shared explosion or flash between uses, a
    /// placed one whose emitter has stopped), or it has no systems we can read.
    pub fn share(&self, materials: &Assets<nvf_assets::NifMaterial>) -> [nvf_assets::release::CacheShare; 2] {
        let mut out: [nvf_assets::release::CacheShare; 2] = Default::default();
        for (k, live) in &self.systems {
            let s = &mut out[usize::from(k.instance.is_none())];
            s.entries += 1;
            s.idle += usize::from(live.iter().flatten().all(|l| l.particles.is_empty()));
            for l in live.iter().flatten() {
                s.images_of(materials, &l.def.material);
            }
        }
        out
    }
}

impl Default for Particles {
    fn default() -> Self {
        Particles {
            systems: HashMap::new(),
            reported: HashSet::new(),
            described: HashSet::new(),
            tally: HashMap::new(),
            placed_seen: HashSet::new(),
            placed_emitted: HashSet::new(),
            instance_refs: HashMap::new(),
            attached_refs: HashMap::new(),
            most_instances: 0,
            rng: SplitMix(0x2545_f491),
            born: 0,
            most_alive: 0,
            basis: None,
        }
    }
}

/// The live systems of placed reference `r`'s models (each model instance's own,
/// particles.md §7.1), for JohnnyGuitar's particle functions (`crate::nif_mirror`).
pub(crate) fn placed_systems(p: &mut Particles, r: u32) -> Vec<&mut ParticleSystemDef> {
    let mine: HashSet<Entity> = p.instance_refs.iter().filter(|(_, &v)| v == r).map(|(e, _)| *e).collect();
    p.systems
        .iter_mut()
        .filter(|(k, _)| k.instance.is_some_and(|e| mine.contains(&e)))
        .filter_map(|(_, v)| v.as_mut())
        .flat_map(|v| v.iter_mut().map(|l| &mut l.def))
        .collect()
}

/// The live systems owned by one concrete emitter entity `owner` (an attached or camera
/// overlay NIF model), isolated from any other emitter on the same reference [design].
pub(crate) fn instance_systems(p: &mut Particles, owner: Entity) -> Vec<&mut ParticleSystemDef> {
    p.systems
        .iter_mut()
        .filter(|(k, _)| k.instance == Some(owner))
        .filter_map(|(_, v)| v.as_mut())
        .flat_map(|v| v.iter_mut().map(|l| &mut l.def))
        .collect()
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn seed_test_system(
    p: &mut Particles,
    owner: Entity,
    reference: u32,
    attached: bool,
    path: &str,
    def: ParticleSystemDef,
    mesh: Handle<Mesh>,
    entity: Entity,
) {
    if attached {
        p.attached_refs.insert(owner, reference);
    } else {
        p.instance_refs.insert(owner, reference);
    }
    let key = Key {
        source: Source::Model(path.to_string()),
        instance: Some(owner),
    };
    p.systems.insert(
        key,
        Some(vec![Live {
            def,
            particles: Vec::new(),
            geometry: ParticleGeometry::default(),
            mesh,
            entity,
            served: 0,
            owed: 0.0,
            camera_local: false,
        }]),
    );
}

/// A system's values by GetNiPSysModifierValue's IDs [documented: the page's table]: 0
/// the order (0, not kept), 1 active, 2-7 the emitter's speed, declination and planar
/// angle with their variations, 8-11 its initial colour, 12-15 radius and life with their
/// variations, 16 the scale, 17-19 a box emitter's width, height and depth; then (20) the
/// spawn rate [hypothesis: the order is unknown, active is whether the controller has
/// not switched it off, the scale the particles' base scale].
pub(crate) fn script_values(d: &ParticleSystemDef) -> [f32; 21] {
    let e = &d.emitter;
    let active = d.active.first().is_none_or(|(_, on)| *on);
    [
        0.0,
        f32::from(u8::from(active)),
        e.speed,
        e.speed_variation,
        e.declination,
        e.declination_variation,
        e.planar_angle,
        e.planar_angle_variation,
        e.color[0],
        e.color[1],
        e.color[2],
        e.color[3],
        e.radius,
        e.radius_variation,
        e.life,
        e.life_variation,
        d.base_scale,
        e.size.x,
        e.size.y,
        e.size.z,
        d.rate,
    ]
}

/// Sets one of [`script_values`] on the live system; the order is not kept.
pub(crate) fn set_script_value(d: &mut ParticleSystemDef, id: usize, v: f32) {
    let e = &mut d.emitter;
    match id {
        1 => d.active = vec![(0.0, v != 0.0)],
        2 => e.speed = v,
        3 => e.speed_variation = v,
        4 => e.declination = v,
        5 => e.declination_variation = v,
        6 => e.planar_angle = v,
        7 => e.planar_angle_variation = v,
        8..=11 => e.color[id - 8] = v,
        12 => e.radius = v,
        13 => e.radius_variation = v,
        14 => e.life = v,
        15 => e.life_variation = v,
        16 => d.base_scale = v,
        17 => e.size.x = v,
        18 => e.size.y = v,
        19 => e.size.z = v,
        20 => {
            d.rate = v;
            d.rate_keys = None;
        }
        _ => {}
    }
}

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        configure_draw_set(app);
        app.init_resource::<Particles>()
            .add_systems(Update, (script_drawn, script_drawn_rig, staged_addons, emit.after(crate::effect_shaders::sync_player_effects), simulate, take_basis).chain().after(crate::shot_fx::ShotFxSet))
            .add_systems(PostUpdate, draw.in_set(ParticleDrawSet))
            .add_systems(Last, report);
    }
}

/// The particle geometry builds (this module's [`draw`], `effect_particles::build`), in
/// `PostUpdate` after this frame's frusta and before visibility propagation and the
/// asset events (module notes, "When").
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParticleDrawSet;

/// Order [`ParticleDrawSet`] (by each plugin that adds to it: the effect shaders' runs
/// without this one too).
pub fn configure_draw_set(app: &mut App) {
    app.configure_sets(
        PostUpdate,
        ParticleDrawSet
            .after(bevy::transform::TransformSystems::Propagate)
            .after(VisibilitySystems::UpdateFrusta)
            .before(VisibilitySystems::VisibilityPropagate)
            .before(AssetEventSystems),
    );
}

/// A reference a script enabled, drawn by nvf-play (`script_draw`): its model's own
/// systems and add-on nodes emit as render streaming's do (`world::spawn_emitters`,
/// particles.md §7.4): the smoke of a burning tyre pile a quest lights. One drawn with
/// its own nodes and named sequences (a `ScriptDrawnRig` on its rig's root, where
/// `object_anims` puts its `GroupRig`) emits its own systems from that root instead
/// ([`script_drawn_rig`]), so its sequences drive them (§6.7, §7.5: the rising B-29).
fn script_drawn(mut commands: Commands, mut models: ModelCtx, added: Query<(Entity, &ScriptDrawnRoot, &ScriptDrawnModel), Added<ScriptDrawnModel>>) {
    for (e, root, path) in &added {
        let Some(model) = models.model(&path.0) else { continue };
        // script_draw's rule for a `ScriptDrawnRig`: a rig with named sequences.
        if model.particles && model.rig.as_ref().is_some_and(|rig| !rig.groups.is_empty()) {
            // Its add-on nodes only here; its own systems are the rig root's.
            let addons_only = nvf_assets::model::PreparedModel { particles: false, ..(*model).clone() };
            crate::world::spawn_emitters(&mut commands, e, root.0, &addons_only, &path.0, Affine3A::IDENTITY);
        } else {
            crate::world::spawn_emitters(&mut commands, e, root.0, &model, &path.0, Affine3A::IDENTITY);
        }
    }
}

/// A script-drawn rig with named sequences: its model's own systems emit from the rig's
/// root, beside its `GroupRig` and the clip playing there (`emit`'s drive, §6.7).
fn script_drawn_rig(mut commands: Commands, mut models: ModelCtx, added: Query<(Entity, &ScriptDrawnRig), Added<ScriptDrawnRig>>) {
    for (e, rig) in &added {
        if models.model(&rig.path).is_some_and(|m| m.particles) {
            commands.entity(e).insert(ModelEmitter::placed(rig.path.clone(), rig.reference));
        }
    }
}

/// An entity's place in game space from this frame's transforms: the chain of local
/// transforms up to the world root.
pub fn game_affine(e: Entity, transforms: &Query<&Transform>, parents: &Query<&ChildOf>, root: Entity) -> Affine3A {
    let mut m = Affine3A::IDENTITY;
    let mut at = Some(e);
    let mut depth = 0;
    while let Some(p) = at.filter(|p| *p != root) {
        if let Ok(t) = transforms.get(p) {
            m = t.compute_affine() * m;
        }
        at = parents.get(p).ok().map(ChildOf::parent);
        depth += 1;
        if depth > 256 {
            break;
        }
    }
    m
}

#[allow(clippy::too_many_arguments)]
fn systems_of<'a>(
    p: &'a mut Particles,
    key: &Key,
    sfx: Option<&Sfx>,
    models: &mut ModelCtx,
    commands: &mut Commands,
    root: Entity,
    idle: Option<&IdleParticleRig>,
    layer: Option<&RenderLayers>,
    camera_local: bool,
) -> Option<&'a mut Vec<Live>> {
    let source = &key.source;
    if !p.systems.contains_key(key) {
        let (path, what) = match source {
            Source::Addon(index) => {
                let addon = sfx.and_then(|s| s.forms.addons.get(index));
                (addon.and_then(|a| a.model.as_deref()).map(fnv_actor::resolve::mesh_path), format!("add-on node {index}"))
            }
            Source::Model(path) => (Some(path.clone()), "model".to_string()),
        };
        let prepared: Option<Arc<PreparedParticles>> = path.as_deref().and_then(|p| models.particles(p));
        let live = prepared.filter(|pp| !pp.systems.is_empty()).map(|pp| {
            if p.described.insert(source.clone()) {
                p.tally.insert(source.clone(), pp.systems.iter().map(|d| (d.name.clone(), 0)).collect());
                // Systems a sequence of the model drives, and those sequences' names.
                let driven = pp.systems.iter().filter(|d| !d.sequences.is_empty()).count();
                let mut names: Vec<&str> = Vec::new();
                for s in pp.systems.iter().flat_map(|d| &d.sequences) {
                    if !names.contains(&s.sequence.as_str()) {
                        names.push(&s.sequence);
                    }
                }
                info!(
                    "particles: {what} {}: {} system(s) [{}]{}{}",
                    path.as_deref().unwrap_or("?"),
                    pp.systems.len(),
                    pp.systems.iter().map(ParticleSystemDef::describe).collect::<Vec<_>>().join("; "),
                    if pp.unread.is_empty() { String::new() } else { format!("; not read: {}", pp.unread.join(", ")) },
                    if driven == 0 { String::new() } else { format!("; {driven} driven by the sequences [{}]", names.join(", ")) }
                );
            }
            pp.systems
                .iter()
                .map(|def| {
                    let mesh = models.meshes.add(particle_mesh());
                    let entity = commands
                        .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(def.material.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, NoFrustumCulling, ChildOf(root)))
                        .id();
                    if let Some(layer) = layer {
                        commands.entity(entity).insert(layer.clone());
                    }
                    if let (Some(idle), Some(owner)) = (idle, key.instance) {
                        if let Some(materials) = def.idle_materials(entity, &idle.clip) {
                            let mut player = Playing::new(&idle.clip.name, &idle.clip, |_| None);
                            player.held = true;
                            commands.entity(entity).insert((IdleParticleMaterial(owner), player, materials));
                        }
                    }
                    let mut def = def.clone();
                    if let Some(idle) = idle {
                        def.bind_idle(&idle.clip);
                    }
                    Live { def, particles: Vec::new(), geometry: ParticleGeometry::default(), mesh, entity, served: 0, owed: 0.0, camera_local }
                })
                .collect()
        });
        // A model without systems of its own is the common case (bullet impacts).
        if live.is_none() && matches!(source, Source::Addon(_)) && p.reported.insert(source.clone()) {
            warn!("particles: {what}: no particle systems ({})", path.as_deref().unwrap_or("no ADDN record or model"));
        }
        p.systems.insert(key.clone(), live);
    }
    p.systems.get_mut(key).and_then(Option::as_mut)
}

/// A number in −1..1.
/// The uniform scale of an emitter's place in the world: the mean length of its axes
/// (1 for a frame that does not scale). A placed reference at scale 0.5 (the Ultra-Luxe
/// fountain's ULFountainFX, issue #1547) gives 0.5.
fn frame_scale(frame: &Affine3A) -> f32 {
    let m = frame.matrix3;
    let k = (m.x_axis.length() + m.y_axis.length() + m.z_axis.length()) / 3.0;
    if k.is_finite() && k > 0.0 {
        k
    } else {
        1.0
    }
}

fn spread(rng: &mut SplitMix) -> f32 {
    rng.next_f32() * 2.0 - 1.0
}

/// A model emitter, its visibility, and the rig and sequence playing when it is drawn
/// with its own nodes.
type ModelEmitterItem = (
    Entity,
    &'static mut ModelEmitter,
    Option<&'static InheritedVisibility>,
    Option<&'static crate::object_anims::GroupRig>,
    Option<&'static IdleParticleRig>,
    Option<&'static Playing>,
    Option<&'static crate::effect_shaders::PlayerEffectEmitter>,
);

/// An emitter to serve this frame: its entity, systems, time, placed reference and drive.
/// An emitter, its systems' key, age, placed reference, the sequence driving its drawn
/// model, and the clips' word on a held model's systems ([`ModelEmitter::driven`]).
/// The emitter, its systems' key, its age, its placed reference, its drive, its held
/// clips' word, the factor on its particles' size ([`ModelEmitter::size`]), optional
/// render layer, and optional local space root (e.g. a camera overlay holder).
type EmitSource = (Entity, Key, f32, Option<u32>, Option<Drive>, Option<HashMap<String, Option<f32>>>, (f32, bool), Option<RenderLayers>, Option<Entity>);

/// Births: each add-on node in the world emits into the systems of its ADDN, each
/// model emitter into its file's own.
#[allow(clippy::too_many_arguments)]
fn emit(
    mut commands: Commands,
    time: Res<Time>,
    mut p: ResMut<Particles>,
    sfx: Option<Res<Sfx>>,
    mut models: ModelCtx,
    root: Query<(Entity, &Transform), With<WorldRoot>>,
    camera: Query<&Transform, crate::fly::WorldCamera>,
    mut emitters: Query<(Entity, &mut AddonEmitter, Option<&InheritedVisibility>, Option<&crate::effect_shaders::PlayerEffectEmitter>)>,
    mut model_emitters: Query<ModelEmitterItem>,
    rigs: Query<&crate::object_anims::GroupRig>,
    idle_rigs: Query<&IdleParticleRig>,
    transforms: Query<&Transform>,
    parents: Query<&ChildOf>,
    body: Option<Res<crate::player_body::PlayerBody>>,
) {
    let world_root = root.single().ok();
    // The camera in game space: emitters nearest it are served first.
    let eye = world_root.and_then(|(_, root_t)| {
        camera.single().ok().map(|c| Vec3A::from(root_t.compute_affine().inverse().transform_point3(c.translation)))
    });
    let dt = time.delta_secs();
    let p = &mut *p;
    for live in p.systems.values_mut().flatten().flatten() {
        live.served = 0;
    }
    let mut sources: Vec<EmitSource> = Vec::new();
    // Placed model instances whose emitter still exists (hidden or not), and those of
    // them hidden this frame.
    let mut present: HashSet<Entity> = HashSet::new();
    let mut hidden_now: HashSet<Entity> = HashSet::new();
    // A hidden emitter (a disabled placed fire, `world::spawn_emitters`) keeps its time
    // but makes nothing. Not in its first frame, before its visibility was worked out:
    // a muzzle flash lives one frame [design].
    let hidden = |age: f32, v: Option<&InheritedVisibility>| age > 0.0 && v.is_some_and(|v| !v.get());
    // [design: effect_shaders::player_addon] A player's hidden part suppresses births
    // immediately, including an emitter's first frame before visibility propagates.
    let part_seen = |part: Option<&crate::effect_shaders::PlayerEffectEmitter>| part.is_none_or(|part| body.as_deref().is_some_and(|b| b.sees_effect(part.0)));
    if world_root.is_some() {
        for (e, mut em, v, part) in &mut emitters {
            if let Some(r) = em.placed {
                p.placed_seen.insert(r);
            }
            if part_seen(part) && !hidden(em.age, v) {
                sources.push((e, Key { source: Source::Addon(em.index), instance: None }, em.age, em.placed, None, None, (1.0, false), None, None));
            }
            em.age += dt;
        }
    }
    for (e, mut em, v, rig, idle_rig, playing, part) in &mut model_emitters {
        if em.space_root.is_none() && world_root.is_none() {
            continue;
        }
        // A placed model's systems are its own (particles.md §7.1).
        let instance = em.placed.map(|_| e);
        if let Some(r) = em.placed {
            p.placed_seen.insert(r);
            present.insert(e);
            if em.attached {
                p.attached_refs.insert(e, r);
            } else {
                p.instance_refs.insert(e, r);
            }
        }
        // Drawn with its own nodes: the sequence playing on it and the span of its time
        // since last frame; a sequence just begun counts from its start, unless it was
        // shown already well into it (drawn at a group's held end).
        let drive = (rig.is_some() || idle_rig.is_some()).then(|| {
            let now = playing.map(|p| (p.name.clone(), (p.time - p.start).max(0.0)));
            let last = em.clock.take();
            let mut began = false;
            let span = now.as_ref().map(|(name, t)| {
                let from = match &last {
                    Some((n, l)) if n == name && *l <= *t => *l,
                    _ => {
                        began = true;
                        if *t <= 0.25 { 0.0 } else { (*t - dt).max(0.0) }
                    }
                };
                (name.clone(), from, *t)
            });
            em.clock = now;
            Drive { rig: e, playing: span, began }
        });
        if em.placed.is_some() && hidden(em.age, v) {
            hidden_now.insert(e);
        }
        // A held model whose clips switch nothing on costs nothing.
        let idle = em.driven.as_ref().is_some_and(HashMap::is_empty);
        if part_seen(part) && !hidden(em.age, v) && !idle {
            sources.push((e, Key { source: Source::Model(em.path.clone()), instance }, em.age, em.placed, drive, em.driven.clone(), (em.size.max(0.0), em.scale_radius_with_holder), em.layer.clone(), em.space_root));
        }
        em.age += dt;
    }
    // A master system serves at most `max_emitters` emitters a frame: the ones nearest the
    // camera [hypothesis, particles.md §7.2], so the fires in view burn when more barrels
    // than that are loaded.
    let places: Vec<Affine3A> = sources.iter().map(|(e, .., space_root)| {
        let sr = space_root.or_else(|| world_root.map(|(r, _)| r)).expect("source root");
        game_affine(*e, &transforms, &parents, sr)
    }).collect();
    let mut order: Vec<usize> = (0..sources.len()).collect();
    if let Some(eye) = eye {
        order.sort_by(|&a, &b| places[a].translation.distance_squared(eye).total_cmp(&places[b].translation.distance_squared(eye)));
    }
    for i in order {
        let (_, key, age, placed, drive, driven, (size, scale_radius_with_holder), layer, space_root) = &sources[i];
        let (age, placed) = (*age, *placed);
        let sr = space_root.or_else(|| world_root.map(|(r, _)| r)).expect("source root");
        let rig = drive.as_ref().and_then(|d| rigs.get(d.rig).ok());
        let mut rng = SplitMix(p.rng.0);
        let Some(systems) = systems_of(p, key, sfx.as_deref(), &mut models, &mut commands, sr, key.instance.and_then(|e| idle_rigs.get(e).ok()), layer.as_ref(), space_root.is_some()) else { continue };
        let node = places[i];
        let size = *size * if *scale_radius_with_holder { frame_scale(&node) } else { 1.0 };
        let mut counts = vec![0u64; systems.len()];
        let mut born = 0;
        if let Some((name, ..)) = drive.as_ref().filter(|d| d.began).and_then(|d| d.playing.as_ref()) {
            let by = systems.iter().filter(|l| l.def.sequence(name).is_some()).count();
            if by > 0 {
                info!("particles: {:08X} {} emits as its {name} says: {by} of {} systems", placed.unwrap_or(0), key.source.label(), systems.len());
            }
        }
        for (k, live) in systems.iter_mut().enumerate() {
            let owed = &mut live.owed;
            let d = &live.def;
            // A drawn model's sequences drive it: by the one playing, on its time, else
            // not at all (particles.md §6.7).
            let by_sequence = drive.as_ref().filter(|_| !d.sequences.is_empty()).map(|dr| dr.playing.as_ref().and_then(|(name, from, to)| d.sequence(name).map(|s| (s, *from, *to))));
            let (on, rate, t, sequence) = match by_sequence {
                Some(None) => continue,
                Some(Some((s, from, to))) => (s.on_time(from, to), s.rate_at((from + to) * 0.5), to, Some(s)),
                None => match driven.as_ref().map(|m| m.get(&d.name).copied()) {
                    // A held model's system: on for the whole frame while the clips say
                    // so, at their rate where they key one, else its own (§7.6).
                    Some(None) => continue,
                    Some(Some(rate)) => (dt, rate.unwrap_or_else(|| d.rate_at(age + dt * 0.5)), age, None),
                    // A keyed rate at the middle of the frame (particles.md §6.6).
                    None => (d.on_time(age, age + dt), d.rate_at(age + dt * 0.5), age, None),
                },
            };
            // The emitter's node as a sequence of its file moves it (a dust devil on its
            // path, particles.md §6.8): on the drawn model's sequence time when one drives
            // the system, else on the emitter's own (`t`, the same clock as the births).
            // Box emitters also take their object's place from the drawn rig below; the
            // mover adds the mesh emitters and the bombs under the moved node.
            let node = node * d.moved(t);
            if on <= 0.0 || (d.max_emitters > 0 && live.served >= d.max_emitters) {
                continue;
            }
            live.served += 1;
            *owed += rate * on;
            // From its emitter object's node as the drawn model's sequence moves it.
            let moved = d.emitter.object.as_deref().filter(|_| d.emitter.mesh.is_none()).and_then(|name| {
                rig.and_then(|r| r.node(name)).or_else(|| drive.as_ref().and_then(|dr| idle_rigs.get(dr.rig).ok()).and_then(|r| r.nodes.get(name).copied()))
            }).map(|e| game_affine(e, &transforms, &parents, sr));
            let frame = moved.unwrap_or(node * d.emitter.frame);
            let (_, turn, origin) = frame.to_scale_rotation_translation();
            // The reference's scale and the system node's set the spray's speed and its
            // gravity (particles.md §7.7).
            let spray = frame_scale(&node) * d.system_scale;
            // The volume is the emitter object's, at that object's scale.
            let volume = frame_scale(&frame);
            let vals = match sequence {
                Some(s) => d.emitter.values_in(t, s),
                None => d.emitter.values(age),
            };
            // Births past the free slots are dropped (as the loop below would skip them);
            // capping here keeps a broken file's huge or infinite rate from looping
            // without end.
            let free = d.max.saturating_sub(live.particles.len()) as f32;
            *owed = if owed.is_finite() { owed.min(free + owed.fract()) } else { 0.0 };
            while *owed >= 1.0 {
                *owed -= 1.0;
                if live.particles.len() >= d.max {
                    continue;
                }
                let m = &d.emitter;
                // Where it starts and the axis of its cone: on a mesh, along the normal
                // there (or at random, or a set axis); else somewhere in the emitter's
                // volume, about its +z.
                let (start, axis) = match &m.mesh {
                    Some(me) => {
                        let mesh = &me.meshes[((rng.next_f32() * me.meshes.len() as f32) as usize).min(me.meshes.len() - 1)];
                        let Some((at, normal)) = mesh.pick(me.from, &mut || rng.next_f32()) else { continue };
                        let mf = node * mesh.frame;
                        let along = match me.velocity {
                            InitialVelocity::Normals => mf.transform_vector3(normal),
                            InitialVelocity::Random => Vec3::new(spread(&mut rng), spread(&mut rng), spread(&mut rng)),
                            InitialVelocity::Direction(a) => mf.transform_vector3(a),
                        };
                        (mf.transform_point3(at), Quat::from_rotation_arc(Vec3::Z, along.normalize_or(Vec3::Z)))
                    }
                    None => {
                        let offset = if m.round {
                            let a = rng.next_f32() * std::f32::consts::TAU;
                            let r = m.size.x * rng.next_f32().sqrt();
                            Vec3::new(a.cos() * r, a.sin() * r, m.size.z * spread(&mut rng) * 0.5)
                        } else {
                            m.size * Vec3::new(spread(&mut rng), spread(&mut rng), spread(&mut rng)) * 0.5
                        };
                        (origin + turn * offset * volume, turn)
                    }
                };
                // Thrown within the cone: so far from the axis, any way round it.
                let decl = vals.declination + vals.declination_variation * spread(&mut rng);
                let planar = vals.planar_angle + vals.planar_angle_variation * spread(&mut rng);
                let dir = Vec3::new(decl.sin() * planar.cos(), decl.sin() * planar.sin(), decl.cos());
                let speed = (vals.speed + vals.speed_variation * spread(&mut rng) * 0.5) * spray;
                let rot = d.rotation.clone().unwrap_or_default();
                let mut spin = rot.speed + rot.speed_variation * spread(&mut rng);
                if rot.random_sign && rng.next_f32() < 0.5 {
                    spin = -spin;
                }
                live.particles.push(Particle {
                    frame: node,
                    pos: start,
                    vel: axis * dir * speed,
                    scale: spray,
                    born_at: t,
                    sequence: sequence.map(|s| s.index),
                    age: 0.0,
                    life: (vals.life + vals.life_variation * spread(&mut rng) * 0.5).max(0.02),
                    // The system node sizes the drops. Placed reference scale affects
                    // the spray but leaves that radius alone (particles.md §3, §7.7).
                    radius: (vals.radius + vals.radius_variation * spread(&mut rng)).max(0.0) * d.system_scale * size,
                    angle: rot.angle + rot.angle_variation * spread(&mut rng),
                    spin,
                    tex: if d.subtextures.is_empty() { 0 } else { ((rng.next_f32() * d.subtextures.len() as f32) as usize).min(d.subtextures.len() - 1) },
                    // A strip starts where its particle is born.
                    trail: if d.strip.is_some() { vec![start] } else { Vec::new() },
                    trail_clock: 0.0,
                });
                born += 1;
                counts[k] += 1;
            }
        }
        p.rng = rng;
        p.born += born;
        if born > 0 {
            if let Some(t) = p.tally.get_mut(&key.source) {
                for (slot, n) in t.iter_mut().zip(&counts) {
                    slot.1 += n;
                }
            }
            if let Some(r) = placed {
                p.placed_emitted.insert(r);
            }
        }
    }
    // A placed model's own particles go at once when it is no longer drawn: its emitter
    // hidden (a script disabled it, the item was taken) or gone (its cell unloaded, the
    // reference's model taken down) [design, #1596: a taken snow globe's snow went on
    // falling where it stood]. Its systems go with an emitter that is gone.
    for (k, live) in p.systems.iter_mut() {
        let Some(e) = k.instance.filter(|e| hidden_now.contains(e) || !present.contains(e)) else { continue };
        let n: usize = live.iter().flatten().map(|s| s.particles.len()).sum();
        if n > 0 {
            info!("particles: {:08X} is not drawn any more: its {n} live particles go", p.instance_refs.get(&e).or_else(|| p.attached_refs.get(&e)).copied().unwrap_or(0));
            for s in live.iter_mut().flatten() {
                s.particles.clear();
            }
        }
    }
    let gone: Vec<Key> = p.systems.keys().filter(|k| k.instance.is_some_and(|e| !present.contains(&e))).cloned().collect();
    p.instance_refs.retain(|e, _| present.contains(e));
    p.attached_refs.retain(|e, _| present.contains(e));
    for k in gone {
        for live in p.systems.remove(&k).flatten().unwrap_or_default() {
            commands.entity(live.entity).try_despawn();
            models.meshes.remove(&live.mesh);
        }
    }
    let instances = p.systems.keys().filter(|k| k.instance.is_some()).count();
    p.most_instances = p.most_instances.max(instances);
}

/// Ageing and movement of every live particle.
fn simulate(time: Res<Time>, mut p: ResMut<Particles>) {
    let dt = time.delta_secs();
    let mut alive = 0;
    for live in p.systems.values_mut().flatten().flatten() {
        let d = &live.def;
        for q in &mut live.particles {
            q.age += dt;
            let at = q.born_at + q.age;
            // Born under a drawn model's sequence: that sequence's keys and length.
            let (seq, period, loops) = match q.sequence.and_then(|i| d.sequences.iter().find(|s| s.index == i)) {
                Some(s) => (Some(s.index), s.period, s.loops),
                None => (None, d.period, d.loops),
            };
            for g in d.gravity.iter().filter(|g| g.active.on_in(seq, at, period, loops)) {
                let centre = q.frame.transform_point3(g.origin);
                let pull = if g.spherical { (centre - q.pos).normalize_or_zero() } else { g.direction };
                // Weaker away from its object, by the distance in the system's own units
                // (those of its speeds and radii: `scale`).
                let fall = g.falloff(q.pos.distance(centre) / q.scale);
                q.vel += pull * g.strength_at(at) * fall * q.scale * dt;
            }
            for b in d.bombs.iter().filter(|b| b.active.on_in(seq, at, period, loops)) {
                q.vel += b.push(&q.frame, q.pos) * dt;
            }
            for drag in d.drag.iter().filter(|g| g.active.on_in(seq, at, period, loops)) {
                // The share of the speed along the axis kept after `dt`.
                // Only near its object, when the file gives it a range.
                // Its range is in the model's units, about its object's place in the file
                // (particles.md §7.7: fitted to the owner's video of the fountain).
                let reach = drag.reach(q.pos.distance(q.frame.transform_point3(drag.origin)) / frame_scale(&q.frame));
                if reach <= 0.0 {
                    continue;
                }
                let keep = (1.0 - drag.percentage.clamp(0.0, 1.0) * reach).powf(dt * DRAG_STEPS_PER_SECOND);
                let along = q.vel.dot(drag.axis);
                q.vel -= drag.axis * along * (1.0 - keep);
            }
            let to = q.pos + q.vel * dt;
            match d.colliders.iter().find_map(|c| c.hit(&q.frame, q.pos, to, q.vel)) {
                None => q.pos = to,
                Some(None) => q.life = q.age,
                Some(Some((at, vel))) => {
                    q.pos = at;
                    q.vel = vel;
                }
            }
            q.angle += q.spin * dt;
            if let Some(st) = d.strip {
                q.trail_clock += dt;
                while q.trail_clock >= st.step {
                    q.trail_clock -= st.step;
                    q.trail.push(q.pos);
                }
                // The recorded points and the particle itself make at most `max_points`.
                let keep = st.max_points.saturating_sub(1).max(1);
                if q.trail.len() > keep {
                    q.trail.drain(..q.trail.len() - keep);
                }
            }
        }
        live.particles.retain(|q| q.age < q.life);
        alive += live.particles.len();
    }
    p.most_alive = p.most_alive.max(alive);
}

/// The world camera the particle draws face: the main view's (`fly::WorldCamera`), never
/// the reflection, scope or menu views' cameras beside it (module notes, "When").
pub use crate::fly::WorldCamera;

/// The main camera's right, up and backward in game space, as the particle draws take
/// them: `rotation` the camera's, `to_game` Bevy's space to the game's (the world root's
/// inverse).
pub fn camera_basis(rotation: Quat, to_game: &Affine3A) -> [Vec3; 3] {
    [
        to_game.transform_vector3(rotation * Vec3::X).normalize_or(Vec3::X),
        to_game.transform_vector3(rotation * Vec3::Y).normalize_or(Vec3::Z),
        to_game.transform_vector3(rotation * Vec3::Z).normalize_or(Vec3::Y),
    ]
}

/// The basis [`draw`] builds with this frame, taken where the draw took it before it moved
/// to `PostUpdate` (module notes, "When"): from the world camera's (`FlyCam`, main.rs
/// `setup`) and the world root's local transforms after `simulate`; none without exactly
/// one of each.
fn take_basis(
    mut p: ResMut<Particles>,
    camera: Query<&Transform, WorldCamera>,
    root: Query<&Transform, With<WorldRoot>>,
) {
    p.basis = match (camera.single(), root.single()) {
        (Ok(cam), Ok(root)) => Some(camera_basis(cam.rotation, &root.compute_affine().inverse())),
        _ => None,
    };
}

/// One mesh per system: a square facing the camera for each particle, built only for a
/// system some camera may see this frame (module notes, "Built only where a camera may
/// see it"); the others' render entities are hidden.
fn draw(
    mut p: ResMut<Particles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut visibility: Query<&mut Visibility>,
    views: ParticleViews,
    mut counts: Option<ResMut<crate::profile::ParticleCull>>,
) {
    // Taken: a frame whose `take_basis` did not run draws no world systems, as before;
    // camera-local overlay systems use the camera's local game-space basis directly.
    let world_basis = p.basis.take();
    let frame = views.frame();
    for live in p.systems.values_mut().flatten().flatten() {
        let Some([right, up, back]) = (if live.camera_local { Some([Vec3::X, Vec3::Z, -Vec3::Y]) } else { world_basis }) else { continue };
        let alive = !live.particles.is_empty();
        let seen = alive && frame.may_see(particle_bounds(&live.def, &live.particles), live.entity, &views);
        if let Ok(mut v) = visibility.get_mut(live.entity) {
            let want = if seen { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
        if !alive {
            // An emptied system keeps only the mesh's set (its last geometry, hidden).
            if live.geometry.positions.capacity() > 0 {
                live.geometry = ParticleGeometry::default();
            }
            continue;
        }
        if let Some(c) = counts.as_deref_mut() {
            c.count(crate::profile::ParticleKind::Regular, seen);
        }
        if !seen {
            // Its mesh keeps its last geometry, hidden, until a camera may see it again.
            continue;
        }
        let Some(mut mesh) = meshes.get_mut(&live.mesh) else { continue };
        live.geometry.rebuild(&live.def, &live.particles, right, up, back);
        let matched = live.geometry.swap_into_mesh(&mut mesh);
        debug_assert!(matched, "particles: {}: mesh layout differs from particle_mesh's; geometry inserted", live.def.name);
    }
}

/// A game-space box holding every vertex [`ParticleGeometry::rebuild`] makes of
/// `particles`, whatever the camera's basis: a square's corners lie within its half size
/// × √2 of the particle (the camera's right and up are orthonormal: the world root only
/// turns, `world::game_to_bevy`), a ribbon's within its half width of each point it passes
/// through (its sides are unit vectors × the half width); ribbons of under two points draw
/// nothing. `None` without a vertex or with a value not finite (then drawn as before).
fn particle_bounds(d: &ParticleSystemDef, particles: &[Particle]) -> Option<Aabb> {
    let half = |q: &Particle| (q.radius * d.scale(q.age, q.life)).abs();
    if d.strip.is_some() {
        sphere_bounds(particles.iter().filter(|q| !q.trail.is_empty()).flat_map(|q| {
            let h = half(q);
            q.trail.iter().copied().chain([q.pos]).map(move |p| (p, h))
        }))
    } else {
        sphere_bounds(particles.iter().map(|q| (q.pos, half(q) * std::f32::consts::SQRT_2)))
    }
}

/// The box holding every sphere (centre, radius), padded so rounding cannot leave a
/// vertex outside: a thousandth of each radius and a hundred-thousandth of its distance
/// from the origin (far more than a float's rounding there, and under a unit at the
/// map's edge). `None` without a sphere or when a value is not finite.
pub fn sphere_bounds(spheres: impl IntoIterator<Item = (Vec3, f32)>) -> Option<Aabb> {
    let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    let mut any = false;
    for (c, r) in spheres {
        let r = r.abs();
        if !c.is_finite() || !r.is_finite() {
            return None;
        }
        let r = r * 1.001 + (c.abs().max_element() + r) * 1e-5;
        lo = lo.min(c - r);
        hi = hi.max(c + r);
        any = true;
    }
    any.then(|| Aabb::from_min_max(lo, hi))
}

/// What the particle builds test their boxes against: every 3D camera with its frustum
/// and layers, and the render entities' placement and layers.
#[derive(SystemParam)]
pub struct ParticleViews<'w, 's> {
    cameras: Query<'w, 's, (&'static Camera, &'static Frustum, Option<&'static RenderLayers>), With<Camera3d>>,
    placed: Query<'w, 's, (&'static GlobalTransform, Option<&'static RenderLayers>)>,
}

impl ParticleViews<'_, '_> {
    /// This frame's active cameras, their frusta as `VisibilitySystems::UpdateFrusta` left
    /// them.
    pub fn frame(&self) -> FrameViews {
        FrameViews::new(self.cameras.iter().filter(|(c, ..)| c.is_active).map(|(_, f, l)| (*f, l.cloned().unwrap_or_default())).collect())
    }
}

/// The active 3D cameras of a frame (frustum, layers), for the particle builds' test.
///
/// Every active `Camera3d` whose layers meet the particle entity's counts (layer 0 when
/// it has none, as the particle entities are spawned) [design: main.rs `setup` layers 0,
/// water, arms, body, decals; reflection.rs `setup_reflection` layers 0, arms, body,
/// active only while water is reflected; scope_view.rs's lens camera layers 0, water,
/// decals while a 3D scope is up; the menu views' cameras draw their own layers and are
/// left out by the layer test, which errs toward drawing should one draw layer 0].
pub struct FrameViews {
    /// The cameras whose frusta are set.
    views: Vec<(Frustum, RenderLayers)>,
    /// No active camera at all: every system counts as seen.
    unsure: bool,
    /// The layers of the cameras whose frustum's sides are not set (a default frustum
    /// rejects everything): a system on one of them counts as seen. Only those systems:
    /// one camera of other layers (a menu view's) not yet set leaves the rest culled.
    unset_layers: RenderLayers,
    layer0: RenderLayers,
}

impl FrameViews {
    pub fn new(views: Vec<(Frustum, RenderLayers)>) -> FrameViews {
        // A side plane whose normal is not clearly unit length (zero, tiny or NaN) means the
        // frustum was never set: NaN compares as None, so it counts as unset too.
        let unset = |f: &Frustum| f.half_spaces[..4].iter().any(|h| h.normal().length_squared().partial_cmp(&0.25) != Some(std::cmp::Ordering::Greater));
        let unsure = views.is_empty();
        let (unset_views, views): (Vec<_>, Vec<_>) = views.into_iter().partition(|(f, _)| unset(f));
        let unset_layers = unset_views.iter().fold(RenderLayers::none(), |all, (_, l)| all.union(l));
        FrameViews { views, unsure, unset_layers, layer0: RenderLayers::default() }
    }

    /// Whether some camera may see `bounds` (game space) of render entity `e`, placed by
    /// its `GlobalTransform` (the world root's turn: game space to Bevy's). True when in
    /// doubt: no bounds, no placement, no camera, a camera drawing its layers unset.
    pub fn may_see(&self, bounds: Option<Aabb>, e: Entity, views: &ParticleViews) -> bool {
        let Some(b) = bounds.filter(|_| !self.unsure) else { return true };
        let Ok((g, layers)) = views.placed.get(e) else { return true };
        let layers = layers.unwrap_or(&self.layer0);
        layers.intersects(&self.unset_layers) || !outside_every_view(&b, &g.affine(), layers, &self.views)
    }
}

/// Whether no view in `views` (frustum, layers) that draws `layers` could see `bounds`
/// placed by `world_from_game`, by Bevy's box test against each frustum's four sides
/// (not its near or far plane: module notes). True with no such view.
fn outside_every_view(bounds: &Aabb, world_from_game: &Affine3A, layers: &RenderLayers, views: &[(Frustum, RenderLayers)]) -> bool {
    views.iter().filter(|(_, l)| l.intersects(layers)).all(|(f, _)| !f.intersects_obb(bounds, world_from_game, false, false))
}

/// The system's keyed material alpha (`ParticleSystemDef::alpha_keys`) at the emitter
/// time particle `q` is drawn at (its birth time plus its age), 1 without keys
/// (particles.md §6.9).
fn keyed_alpha(d: &ParticleSystemDef, q: &Particle) -> f32 {
    d.alpha_keys.as_ref().and_then(|k| k.at(q.born_at + q.age)).map_or(1.0, |a| a.clamp(0.0, 1.0))
}

/// A particle's colour (the colour modifier's times the emitter's) taken as display
/// values: converted to linear, the alpha kept. The particle systems are unlit
/// (BSShaderNoLightingProperty), and an unlit shape's texture × vertex colour is formed
/// in display values [hypothesis, as `nvf_assets::nif_spawn::unlit_vertex_colors` for
/// placed smoke, nif.md §6 "Vertex colours"]: multiplied in as linear values, the impact
/// dust's 0.27–0.43 greys drew as 0.57–0.69, a pale cloud on every wall hit (owner,
/// playtest list 5 item 48; particles.md §2 "Simple colour").
fn display_colour(c: [f32; 4]) -> [f32; 4] {
    let l = |v: f32| Color::srgb(v.max(0.0), 0.0, 0.0).to_linear().red;
    [l(c[0]), l(c[1]), l(c[2]), c[3]]
}

fn report(mut exit: MessageReader<AppExit>, p: Res<Particles>) {
    if exit.read().last().is_some() && p.born > 0 {
        info!("particles: {} born in {} add-on systems, at most {} alive at once", p.born, p.systems.values().flatten().count(), p.most_alive);
        // Each system that emitted, by its file (all instances of a placed model together).
        for (source, systems) in &p.tally {
            let born: Vec<String> = systems.iter().filter(|(_, n)| *n > 0).map(|(name, n)| format!("{name} {n}")).collect();
            if !born.is_empty() {
                info!("particles: born in {}: {}", source.label(), born.join(", "));
            }
        }
        info!(
            "particles: placed references emitting: {} of {} with particle systems, at most {} placed model instances at once",
            p.placed_emitted.len(),
            p.placed_seen.len(),
            p.most_instances
        );
    }
}

/// The resources a `ModelCtx` reads, empty: no game data (a data folder that does not
/// exist), for tests that run `emit` or the effect particles' births.
#[cfg(test)]
pub(crate) fn model_ctx_resources(world: &mut World) {
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<nvf_assets::NifMaterial>>();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>();
    world.init_resource::<nvf_assets::ModelCache>();
    world.init_resource::<nvf_assets::texture::TextureCache>();
    let vfs = fnv_vfs::Vfs::open(std::path::Path::new("nvf-test-no-game-data"), &[]).expect("an empty file system");
    world.insert_resource(nvf_assets::GameData::new(Arc::new(vfs), Arc::new(fnv_nif::Schema::default())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use nvf_assets::particles::{Emitter, Strip};

    fn test_def(strip: Option<Strip>) -> ParticleSystemDef {
        ParticleSystemDef {
            name: "test".into(),
            max: 0,
            material: default(),
            look: nvf_assets::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
            emitter: Emitter {
                frame: Affine3A::IDENTITY,
                size: Vec3::ZERO,
                round: false,
                speed: 0.0,
                speed_variation: 0.0,
                declination: 0.0,
                declination_variation: 0.0,
                planar_angle: 0.0,
                planar_angle_variation: 0.0,
                color: [1.0; 4],
                radius: 1.0,
                radius_variation: 0.0,
                life: 1.0,
                life_variation: 0.0,
                mesh: None,
                keyed: Vec::new(),
                object: None,
            },
            rate: 0.0,
            rate_keys: None,
            active: Vec::new(),
            period: 0.0,
            loops: false,
            sequences: Vec::new(),
            max_emitters: 0,
            grow_time: 0.0,
            fade_time: 0.0,
            base_scale: 1.0,
            system_scale: 1.0,
            color: None,
            color_keys: None,
            rotation: None,
            gravity: Vec::new(),
            drag: Vec::new(),
            bombs: Vec::new(),
            colliders: Vec::new(),
            strip,
            subtextures: vec![[0.25, 0.5, 0.125, 0.75]],
            mover: None,
            alpha_keys: None,
            soft: 0.0,
        }
    }

    #[test]
    fn an_emitters_scale_is_the_mean_length_of_its_axes() {
        assert_eq!(frame_scale(&Affine3A::IDENTITY), 1.0);
        let half = Affine3A::from_scale_rotation_translation(Vec3::splat(0.5), Quat::from_rotation_z(1.0), Vec3::new(1176.0, -222.0, 1012.0));
        assert!((frame_scale(&half) - 0.5).abs() < 1e-5);
        // A frame squashed to nothing sizes nothing down to zero: 1.
        assert_eq!(frame_scale(&Affine3A::from_scale(Vec3::ZERO)), 1.0);
    }

    #[test]
    fn placed_scale_leaves_radius_alone_and_grass_fire_radius_stays_linear() {
        let emit_radius = |holder_scale: f32, emitter_size: f32, grass_fire: bool| {
            let mut s = Scene::emitting(None, Quat::IDENTITY);
            let emitter = s.world.query_filtered::<Entity, With<ModelEmitter>>().single(&s.world).unwrap();
            *s.world.get_mut::<Transform>(emitter).unwrap() = Transform::from_translation(AHEAD).with_scale(Vec3::splat(holder_scale));
            {
                let mut em = s.world.get_mut::<ModelEmitter>(emitter).unwrap();
                em.size = emitter_size;
                em.scale_radius_with_holder = grass_fire;
                if grass_fire {
                    em.placed = None;
                }
            }
            {
                let mut p = s.world.resource_mut::<Particles>();
                let (mut key, mut systems) = p.systems.drain().next().unwrap();
                let live = systems.iter_mut().flatten().next().unwrap();
                live.def.emitter.radius = 12.0;
                live.def.emitter.radius_variation = 0.0;
                live.def.system_scale = 0.5;
                if grass_fire {
                    key.instance = None;
                }
                p.systems.insert(key, systems);
            }
            s.frame(0.05);
            s.live().particles[0].radius
        };

        // System node scale and the explicit emitter size still apply to placed references.
        let unscaled = emit_radius(1.0, 0.5, false);
        assert_eq!(unscaled, 3.0);
        assert_eq!(emit_radius(0.1, 0.5, false), unscaled);

        let flame_size = 1.2;
        let start = crate::grass_fire_fx::START_SIZE;
        assert!((emit_radius(start, flame_size, true) - 6.0 * flame_size * start).abs() < 1e-5);
        for share in [start, 0.5, 1.0] {
            let (holder, em_size) = crate::grass_fire_fx::flame_scales(flame_size, 1.0, share);
            assert!((emit_radius(holder, em_size, true) - 6.0 * flame_size * share).abs() < 1e-5);
        }
    }

    fn test_particle(pos: Vec3, trail: Vec<Vec3>) -> Particle {
        Particle {
            frame: Affine3A::IDENTITY,
            pos,
            vel: Vec3::ZERO,
            scale: 1.0,
            born_at: 0.0,
            sequence: None,
            age: 0.25,
            life: 1.0,
            radius: 2.0,
            angle: 0.25,
            spin: 0.0,
            tex: 0,
            trail,
            trail_clock: 0.0,
        }
    }

    /// The pre-buffer-reuse construction path, retained only as a differential oracle.
    fn reference_geometry(d: &ParticleSystemDef, particles: &[Particle], right: Vec3, up: Vec3, back: Vec3) -> ParticleGeometry {
        let n = particles.len();
        let mut positions = Vec::with_capacity(n * 4);
        let mut uvs = Vec::with_capacity(n * 4);
        let mut colors = Vec::with_capacity(n * 4);
        let mut indices = Vec::with_capacity(n * 6);
        if d.strip.is_some() {
            for q in particles {
                let points: Vec<Vec3> = q.trail.iter().copied().chain([q.pos]).collect();
                let count = points.len();
                if count < 2 {
                    continue;
                }
                let t = (q.age / q.life).clamp(0.0, 1.0);
                let half = q.radius * d.scale(q.age, q.life);
                let [u, w, v, h] = d.subtextures.get(q.tex).copied().unwrap_or([0.0, 1.0, 0.0, 1.0]);
                let tint = d.tint(t);
                let e = d.emitter.color;
                let colour = display_colour([tint[0] * e[0], tint[1] * e[1], tint[2] * e[2], tint[3] * e[3] * keyed_alpha(d, q)]);
                let base = positions.len() as u32;
                for (i, p) in points.iter().enumerate() {
                    let tangent = points[(i + 1).min(count - 1)] - points[i.saturating_sub(1)];
                    let side = tangent.cross(back).normalize_or(right) * half;
                    positions.extend([(*p - side).to_array(), (*p + side).to_array()]);
                    let along = v + h * (1.0 - i as f32 / (count - 1) as f32);
                    uvs.extend([[u, along], [u + w, along]]);
                    colors.extend([colour; 2]);
                }
                for i in 0..(count as u32 - 1) {
                    let a = base + i * 2;
                    indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
                }
            }
        }
        for (k, q) in particles.iter().enumerate().filter(|_| d.strip.is_none()) {
            let t = (q.age / q.life).clamp(0.0, 1.0);
            let half = q.radius * d.scale(q.age, q.life);
            let (s, c) = q.angle.sin_cos();
            let (x, y) = ((right * c + up * s) * half, (up * c - right * s) * half);
            for corner in [-x - y, x - y, x + y, -x + y] {
                positions.push((q.pos + corner).to_array());
            }
            let [u, w, v, h] = d.subtextures.get(q.tex).copied().unwrap_or([0.0, 1.0, 0.0, 1.0]);
            uvs.extend([[u, v + h], [u + w, v + h], [u + w, v], [u, v]]);
            let tint = d.tint(t);
            let e = d.emitter.color;
            colors.extend([display_colour([tint[0] * e[0], tint[1] * e[1], tint[2] * e[2], tint[3] * e[3] * keyed_alpha(d, q)]); 4]);
            let b = (k * 4) as u32;
            indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        ParticleGeometry { normals: vec![back.to_array(); positions.len()], positions, uvs, colors, indices, points: Vec::new() }
    }

    fn assert_geometry_eq(actual: &ParticleGeometry, expected: &ParticleGeometry) {
        assert_eq!(actual.positions, expected.positions);
        assert_eq!(actual.normals, expected.normals);
        assert_eq!(actual.uvs, expected.uvs);
        assert_eq!(actual.colors, expected.colors);
        assert_eq!(actual.indices, expected.indices);
    }

    #[test]
    fn particle_colours_are_display_values() {
        let c = display_colour([0.5, 1.0, 0.0, 0.3]);
        assert!((c[0] - 0.214).abs() < 1e-3 && (c[1] - 1.0).abs() < 1e-6 && c[2] == 0.0 && c[3] == 0.3, "{c:?}");
    }

    #[test]
    fn billboard_geometry_matches_reference_across_growth_and_shrink() {
        let d = test_def(None);
        let particles = vec![test_particle(Vec3::new(1.0, 2.0, 3.0), Vec::new()), test_particle(Vec3::new(-2.0, 1.0, 0.0), Vec::new())];
        let (right, up, back) = (Vec3::X, Vec3::Z, Vec3::Y);
        let mut actual = ParticleGeometry::default();
        actual.rebuild(&d, &particles, right, up, back);
        assert_geometry_eq(&actual, &reference_geometry(&d, &particles, right, up, back));
        let capacities = (actual.positions.capacity(), actual.normals.capacity(), actual.uvs.capacity(), actual.colors.capacity(), actual.indices.capacity());

        let smaller = &particles[..1];
        actual.rebuild(&d, smaller, right, up, back);
        assert_geometry_eq(&actual, &reference_geometry(&d, smaller, right, up, back));
        assert!(actual.positions.capacity() >= capacities.0 && actual.normals.capacity() >= capacities.1 && actual.uvs.capacity() >= capacities.2 && actual.colors.capacity() >= capacities.3 && actual.indices.capacity() >= capacities.4);
    }

    #[test]
    fn strip_geometry_matches_reference_at_empty_and_one_point_boundaries() {
        let d = test_def(Some(Strip { max_points: 4, step: 0.1 }));
        let (right, up, back) = (Vec3::X, Vec3::Z, Vec3::Y);
        let mut actual = ParticleGeometry::default();

        actual.rebuild(&d, &[], right, up, back);
        assert_geometry_eq(&actual, &reference_geometry(&d, &[], right, up, back));

        let one_point = [test_particle(Vec3::new(1.0, 0.0, 0.0), Vec::new())];
        actual.rebuild(&d, &one_point, right, up, back);
        assert_geometry_eq(&actual, &reference_geometry(&d, &one_point, right, up, back));
        assert!(actual.positions.is_empty() && actual.indices.is_empty());

        let trail = [test_particle(Vec3::new(1.0, 0.0, 0.0), vec![Vec3::ZERO])];
        actual.rebuild(&d, &trail, right, up, back);
        assert_geometry_eq(&actual, &reference_geometry(&d, &trail, right, up, back));
        assert_eq!((actual.positions.len(), actual.indices.len()), (4, 6));
    }

    #[test]
    fn mesh_swaps_publish_current_geometry_and_reuse_both_capacity_sets() {
        fn assert_mesh(mesh: &Mesh, expected: &ParticleGeometry) {
            assert_eq!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap(), expected.positions);
            assert_eq!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap(), expected.normals);
            let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("UV type") };
            let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("colour type") };
            let Some(Indices::U32(indices)) = mesh.indices() else { panic!("index type") };
            assert_eq!(uvs, &expected.uvs);
            assert_eq!(colors, &expected.colors);
            assert_eq!(indices, &expected.indices);
        }
        fn capacities(g: &ParticleGeometry) -> [usize; 5] {
            [g.positions.capacity(), g.normals.capacity(), g.uvs.capacity(), g.colors.capacity(), g.indices.capacity()]
        }
        for strip in [None, Some(Strip { max_points: 4, step: 0.1 })] {
            let d = test_def(strip);
            let particles = [
                test_particle(Vec3::new(1.0, 2.0, 3.0), vec![Vec3::ZERO, Vec3::X]),
                test_particle(Vec3::new(-2.0, 1.0, 0.0), vec![Vec3::Y]),
                test_particle(Vec3::new(2.0, -1.0, 0.0), Vec::new()),
            ];
            let mut geometry = ParticleGeometry::default();
            let mut mesh = particle_mesh();
            // Warm both storage sets before shrinking and growing again (an empty system
            // is not rebuilt: `draw` hides it and drops the scratch set).
            for _ in 0..3 {
                assert!(geometry.rebuild_and_swap(&d, &particles, Vec3::X, Vec3::Z, Vec3::Y, &mut mesh));
            }
            let retained = capacities(&geometry);
            for (frame, count) in [1, 3, 2, 3, 1, 3].into_iter().enumerate() {
                let rotation = Quat::from_rotation_z(frame as f32 * 0.37);
                let (right, up, back) = (rotation * Vec3::X, Vec3::Z, rotation * Vec3::Y);
                let expected = reference_geometry(&d, &particles[..count], right, up, back);
                assert!(geometry.rebuild_and_swap(&d, &particles[..count], right, up, back, &mut mesh));
                assert_mesh(&mesh, &expected);
                assert_eq!(capacities(&geometry), retained);
            }
        }
    }

    impl ParticleGeometry {
        fn rebuild_and_swap(&mut self, d: &ParticleSystemDef, particles: &[Particle], right: Vec3, up: Vec3, back: Vec3, mesh: &mut Mesh) -> bool {
            self.rebuild(d, particles, right, up, back);
            self.swap_into_mesh(mesh)
        }
    }

    /// Two squares worked by hand: camera right X, up Z, back Y, no spin, radius 2, the
    /// subtexture u 0.25 + 0.5, v 0.125 + 0.75.
    #[test]
    fn billboard_geometry_by_hand() {
        let d = test_def(None);
        let mut a = test_particle(Vec3::new(1.0, 2.0, 3.0), Vec::new());
        a.angle = 0.0;
        let mut b = test_particle(Vec3::ZERO, Vec::new());
        b.angle = 0.0;
        let mut g = ParticleGeometry::default();
        g.rebuild(&d, &[a, b], Vec3::X, Vec3::Z, Vec3::Y);
        assert_eq!(&g.positions[..4], &[[-1.0, 2.0, 1.0], [3.0, 2.0, 1.0], [3.0, 2.0, 5.0], [-1.0, 2.0, 5.0]]);
        assert_eq!(&g.positions[4..], &[[-2.0, 0.0, -2.0], [2.0, 0.0, -2.0], [2.0, 0.0, 2.0], [-2.0, 0.0, 2.0]]);
        assert_eq!(&g.uvs[..4], &[[0.25, 0.875], [0.75, 0.875], [0.75, 0.125], [0.25, 0.125]]);
        assert_eq!(g.indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        assert_eq!(g.normals, vec![[0.0, 1.0, 0.0]; 8]);
        assert_eq!(g.colors, vec![[1.0; 4]; 8]);
    }

    /// A two-point ribbon from the origin to X: its sides 2 either way along Z (X × Y),
    /// the oldest point at the subtexture's bottom.
    #[test]
    fn ribbon_geometry_by_hand() {
        let d = test_def(Some(Strip { max_points: 4, step: 0.1 }));
        let mut g = ParticleGeometry::default();
        g.rebuild(&d, &[test_particle(Vec3::X, vec![Vec3::ZERO])], Vec3::X, Vec3::Z, Vec3::Y);
        assert_eq!(g.positions, [[0.0, 0.0, -2.0], [0.0, 0.0, 2.0], [1.0, 0.0, -2.0], [1.0, 0.0, 2.0]]);
        assert_eq!(g.uvs, [[0.25, 0.875], [0.75, 0.875], [0.25, 0.125], [0.75, 0.125]]);
        assert_eq!(g.indices, [0, 1, 3, 0, 3, 2]);
    }

    /// A mesh without `particle_mesh`'s layout (a missing attribute, another type, other
    /// indices) gets the geometry inserted, and the swap says so.
    #[test]
    fn mesh_with_another_layout_gets_geometry_inserted() {
        let d = test_def(None);
        let particles = [test_particle(Vec3::ZERO, Vec::new()), test_particle(Vec3::X, Vec::new())];
        let expected = reference_geometry(&d, &particles, Vec3::X, Vec3::Z, Vec3::Y);
        let mut mesh = particle_mesh();
        mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
        // (insert_attribute refuses a wrong format, so the colours are changed in place.)
        *mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR).unwrap() = VertexAttributeValues::Float32x3(vec![[0.0; 3]]);
        mesh.insert_indices(Indices::U16(vec![0]));
        let mut g = ParticleGeometry::default();
        assert!(!g.rebuild_and_swap(&d, &particles, Vec3::X, Vec3::Z, Vec3::Y, &mut mesh));
        assert_eq!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap(), expected.positions);
        assert_eq!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap(), expected.normals);
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("colour type") };
        assert_eq!(colors, &expected.colors);
        let Some(Indices::U32(indices)) = mesh.indices() else { panic!("index type") };
        assert_eq!(indices, &expected.indices);
        // Once inserted the layout matches, and the next frame swaps again.
        assert!(g.rebuild_and_swap(&d, &particles, Vec3::X, Vec3::Z, Vec3::Y, &mut mesh));
    }

    /// After a burst, a set drawing a quarter or less of its capacity gives memory back.
    #[test]
    fn burst_capacity_is_given_back() {
        let d = test_def(None);
        let burst: Vec<Particle> = (0..256).map(|i| test_particle(Vec3::splat(i as f32), Vec::new())).collect();
        let mut g = ParticleGeometry::default();
        let mut mesh = particle_mesh();
        for _ in 0..2 {
            g.rebuild_and_swap(&d, &burst, Vec3::X, Vec3::Z, Vec3::Y, &mut mesh);
        }
        assert!(g.positions.capacity() >= 1024);
        for _ in 0..2 {
            g.rebuild_and_swap(&d, &burst[..4], Vec3::X, Vec3::Z, Vec3::Y, &mut mesh);
        }
        // Both sets (the scratch one and the mesh's) are back to twice four squares.
        assert!(g.positions.capacity() <= 32 && g.indices.capacity() <= 48, "{} {}", g.positions.capacity(), g.indices.capacity());
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("position type") };
        assert!(positions.capacity() <= 32);
    }

    // --- Built only where a camera may see it (issue #1417) ---

    /// The game's 75° view with Bevy's reverse-z projection, near plane 5 units out.
    fn test_projection() -> Projection {
        Projection::Perspective(PerspectiveProjection { fov: 75f32.to_radians(), aspect_ratio: 16.0 / 9.0, near: 5.0, ..default() })
    }

    /// A camera's transform and the frustum `VisibilitySystems::UpdateFrusta` gives it.
    fn placed_camera(t: Transform, projection: &Projection) -> (Transform, GlobalTransform, Frustum) {
        let g = GlobalTransform::from(t);
        (t, g, projection.compute_frustum(&g))
    }

    /// The bounds hold every vertex drawn, squares and ribbons, whatever way the camera
    /// faces (its basis turned by the world root as the draw turns it), near the origin
    /// and at the map's edge.
    #[test]
    fn particle_bounds_hold_every_vertex_for_any_camera() {
        let to_game = crate::world::game_to_bevy().compute_affine().inverse();
        for strip in [None, Some(Strip { max_points: 4, step: 0.1 })] {
            let d = test_def(strip);
            let mut wide = test_particle(Vec3::new(-3.0, 5.0, 1.0), vec![Vec3::new(-6.0, 5.0, 1.0), Vec3::new(-5.0, 9.0, 3.0)]);
            wide.radius = 7.5;
            let mut far = test_particle(Vec3::new(-71234.5, 60321.25, 8700.75), vec![Vec3::new(-71230.0, 60320.0, 8702.0)]);
            far.radius = 0.3;
            far.angle = 1.1;
            let particles = [test_particle(Vec3::new(1.0, 2.0, 3.0), vec![Vec3::ZERO, Vec3::X]), wide, far, test_particle(Vec3::new(2.0, -1.0, 0.0), Vec::new())];
            let b = particle_bounds(&d, &particles).expect("bounds");
            let (lo, hi) = (Vec3::from(b.min()), Vec3::from(b.max()));
            for k in 0..48 {
                let rot = Quat::from_euler(EulerRot::YXZ, k as f32 * 0.61, (k as f32 * 0.37).sin() * 1.5, k as f32 * 0.11);
                let [right, up, back] = camera_basis(rot, &to_game);
                let mut g = ParticleGeometry::default();
                g.rebuild(&d, &particles, right, up, back);
                assert!(!g.positions.is_empty());
                for p in &g.positions {
                    let p = Vec3::from_array(*p);
                    assert!(p.cmpge(lo).all() && p.cmple(hi).all(), "{p} outside {lo}..{hi} (strip {})", strip.is_some());
                }
            }
        }
        assert!(particle_bounds(&test_def(None), &[]).is_none());
        // A ribbon of one point draws nothing: no bounds, drawn (nothing) as before.
        assert!(particle_bounds(&test_def(Some(Strip { max_points: 4, step: 0.1 })), &[test_particle(Vec3::ZERO, Vec::new())]).is_none());
        let mut lost = test_particle(Vec3::ZERO, Vec::new());
        lost.pos.x = f32::NAN;
        assert!(particle_bounds(&test_def(None), &[lost]).is_none(), "not finite: no bounds");
    }

    /// The camera test: a box well inside a view, behind it, past a side plane, across the
    /// left plane, between the camera and its near plane (seen: the near plane is not
    /// tested), straddling the camera; only views drawing the box's layers count.
    #[test]
    fn boxes_inside_outside_straddling_and_short_of_the_near_plane() {
        // A camera at Bevy's origin looking down -z (game +y), 90° across both ways: at
        // 100 units ahead the left plane is at x = -100. Near plane 5 units out.
        let proj = Projection::Perspective(PerspectiveProjection { fov: std::f32::consts::FRAC_PI_2, aspect_ratio: 1.0, near: 5.0, ..default() });
        let (_, _, frustum) = placed_camera(Transform::IDENTITY, &proj);
        let views = [(frustum, RenderLayers::default())];
        let root = crate::world::game_to_bevy().compute_affine();
        let at = |x: f32, ahead: f32, half: f32| Aabb::from_min_max(Vec3::new(x, ahead, 0.0) - half, Vec3::new(x, ahead, 0.0) + half);
        let layer0 = RenderLayers::default();
        let out = |b: Aabb| outside_every_view(&b, &root, &layer0, &views);
        assert!(!out(at(0.0, 100.0, 1.0)), "ahead");
        assert!(!out(at(0.0, 100_000.0, 1.0)), "far ahead: no far plane");
        assert!(out(at(0.0, -100.0, 1.0)), "behind");
        assert!(out(at(-300.0, 100.0, 1.0)), "far left");
        assert!(out(Aabb::from_min_max(Vec3::new(-1.0, 99.0, 149.0), Vec3::new(1.0, 101.0, 151.0))), "above the top plane");
        assert!(!out(at(-101.0, 100.0, 3.0)), "across the left plane");
        // Between the camera and the near plane (1-3 units out, near 5): nothing of it is
        // drawn, but the test does not look at the near plane: seen (conservative).
        assert!(!out(at(0.0, 2.0, 1.0)), "short of the near plane counts as seen");
        assert!(!out(at(0.0, 0.0, 1.0)), "around the camera");
        assert!(out(at(0.0, -3.0, 1.0)), "just behind the camera");
        // Only views drawing the box's layers count; none at all: no camera sees it.
        let other = [(frustum, RenderLayers::layer(5))];
        assert!(outside_every_view(&at(0.0, 100.0, 1.0), &root, &layer0, &other));
        assert!(!outside_every_view(&at(0.0, 100.0, 1.0), &root, &RenderLayers::from_layers(&[0, 5]), &other));
        assert!(outside_every_view(&at(0.0, 100.0, 1.0), &root, &layer0, &[]));
    }

    /// The water reflection camera's oblique near plane (the water's clip plane) culls
    /// nothing: a box the mirrored camera faces counts as seen on either side of it, and a
    /// box behind the mirrored camera does not.
    #[test]
    fn the_reflections_oblique_near_plane_culls_nothing() {
        // The main camera 100 units over water at Bevy height 0, looking 30° down; the
        // reflection camera mirrored under the water, as `update_reflection` places it.
        let main = Transform::from_xyz(0.0, 100.0, 0.0).with_rotation(Quat::from_rotation_x(-30f32.to_radians()));
        let mirror = Mat4::from_mat3a(bevy::math::reflection_matrix(Vec3::Y));
        let t = Transform::from_matrix(mirror * main.to_matrix());
        let normal = (main.rotation.inverse() * Vec3::NEG_Y).normalize();
        let Projection::Perspective(p) = test_projection() else { unreachable!() };
        let oblique = Projection::Perspective(PerspectiveProjection { near_clip_plane: normal.extend(-1.0 - 100.0), ..p });
        let (_, _, frustum) = placed_camera(t, &oblique);
        let views = [(frustum, RenderLayers::default())];
        let root = crate::world::game_to_bevy().compute_affine();
        // Game (x, y, z) is Bevy (x, z, -y): 300 units ahead of the camera, above and
        // below the water.
        let box_at = |game: Vec3| Aabb::from_min_max(game - 2.0, game + 2.0);
        let layer0 = RenderLayers::default();
        assert!(!outside_every_view(&box_at(Vec3::new(0.0, 300.0, 20.0)), &root, &layer0, &views), "above the water, ahead");
        assert!(!outside_every_view(&box_at(Vec3::new(0.0, 300.0, -20.0)), &root, &layer0, &views), "below the water, ahead: seen (conservative)");
        assert!(outside_every_view(&box_at(Vec3::new(0.0, -300.0, 20.0)), &root, &layer0, &views), "behind");
    }

    /// A world holding the draw's inputs: the world root, the main camera at Bevy's origin
    /// turned by `main`, and one system of `particles` with its render entity.
    struct Scene {
        world: World,
        system: Entity,
        mesh: Handle<Mesh>,
        main: Entity,
        /// The frames run `emit` too (an emitter of the system: [`Scene::emitting`]).
        emits: bool,
    }

    /// What a test camera is: the world camera (`FlyCam`), the water reflection camera, or
    /// another (a scope's, a menu view's).
    #[derive(Clone, Copy, PartialEq)]
    enum Role {
        World,
        Reflection,
        Other,
    }

    /// Game space straight ahead of a camera with no turn (game +y is Bevy -z).
    const AHEAD: Vec3 = Vec3::new(0.0, 500.0, 0.0);
    /// A turn that faces a camera the other way.
    fn away() -> Quat {
        Quat::from_rotation_y(std::f32::consts::PI)
    }

    fn moving_particles() -> Vec<Particle> {
        (0..6)
            .map(|i| {
                let mut q = test_particle(AHEAD + Vec3::new(i as f32 * 20.0, 0.0, 10.0), Vec::new());
                q.vel = Vec3::new(3.0, -2.0, 5.0 + i as f32);
                q.spin = 0.7;
                q.life = 50.0;
                q
            })
            .collect()
    }

    impl Scene {
        fn new(particles: Vec<Particle>, strip: Option<Strip>, main: Quat) -> Scene {
            let mut world = World::new();
            world.init_resource::<Assets<Mesh>>();
            world.insert_resource(Time::<()>::default());
            let root_t = crate::world::game_to_bevy();
            world.spawn((WorldRoot, root_t, GlobalTransform::from(root_t)));
            let main = Scene::camera(&mut world, main, true, None, Role::World);
            let mesh = world.resource_mut::<Assets<Mesh>>().add(particle_mesh());
            let system = world.spawn((Visibility::Hidden, Transform::IDENTITY, GlobalTransform::from(root_t))).id();
            let mut p = Particles::default();
            let live = Live { def: test_def(strip), particles, geometry: ParticleGeometry::default(), mesh: mesh.clone(), entity: system, served: 0, owed: 0.0, camera_local: false };
            p.systems.insert(Key { source: Source::Model("test.nif".into()), instance: None }, Some(vec![live]));
            world.insert_resource(p);
            Scene { world, system, mesh, main, emits: false }
        }

        /// A placed model's emitter ahead of the camera feeding the system (no particle at
        /// first): births at 80 a second, lives of 0.1-0.4 s, so particles are born and die
        /// all the time. `emit` finds the system already made for its key.
        fn emitting(strip: Option<Strip>, main: Quat) -> Scene {
            let mut s = Scene::new(Vec::new(), strip, main);
            model_ctx_resources(&mut s.world);
            let root = s.world.query_filtered::<Entity, With<WorldRoot>>().single(&s.world).unwrap();
            let emitter = s.world.spawn((ModelEmitter::placed("test.nif", 0x00AB_CDEF), Transform::from_translation(AHEAD), ChildOf(root))).id();
            let mut p = s.world.resource_mut::<Particles>();
            let (_, mut systems) = p.systems.drain().next().unwrap();
            for live in systems.iter_mut().flatten() {
                let d = &mut live.def;
                d.max = 120;
                d.rate = 80.0;
                d.emitter.life = 0.25;
                d.emitter.life_variation = 0.3;
                d.emitter.speed = 40.0;
                d.emitter.speed_variation = 20.0;
                d.emitter.size = Vec3::splat(10.0);
                d.emitter.declination_variation = 0.5;
                d.emitter.radius = 3.0;
                d.emitter.radius_variation = 1.0;
            }
            p.systems.insert(Key { source: Source::Model("test.nif".into()), instance: Some(emitter) }, systems);
            s.emits = true;
            s
        }

        /// A 3D camera at Bevy's origin turned by `rotation`, of `role` (only the world
        /// camera gives the draw its basis, as `take_basis` finds it).
        fn camera(world: &mut World, rotation: Quat, active: bool, layers: Option<RenderLayers>, role: Role) -> Entity {
            let (t, g, f) = placed_camera(Transform::from_rotation(rotation), &test_projection());
            let mut e = world.spawn((Camera3d::default(), Camera { is_active: active, ..default() }, test_projection(), t, g, f));
            if let Some(l) = layers {
                e.insert(l);
            }
            match role {
                Role::World => {
                    e.insert(crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 });
                }
                Role::Reflection => {
                    e.insert(crate::reflection::ReflectionCamera);
                }
                Role::Other => {}
            }
            e.id()
        }

        /// Turn camera `e` (its transform and frustum, as this frame's would be).
        fn turn(&mut self, e: Entity, rotation: Quat) {
            let (t, g, f) = placed_camera(Transform::from_rotation(rotation), &test_projection());
            self.world.entity_mut(e).insert((t, g, f));
        }

        /// One frame: emit (an emitting scene), simulate (after `dt`), take the basis, draw.
        fn frame(&mut self, dt: f32) {
            self.world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(dt));
            if self.emits {
                self.world.run_system_once(emit).unwrap();
            }
            self.world.run_system_once(simulate).unwrap();
            self.world.run_system_once(take_basis).unwrap();
            self.world.run_system_once(draw).unwrap();
        }

        fn shown(&self) -> bool {
            *self.world.get::<Visibility>(self.system).unwrap() == Visibility::Inherited
        }

        fn mesh_positions(&self) -> Vec<[f32; 3]> {
            self.world.resource::<Assets<Mesh>>().get(&self.mesh).unwrap().attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().to_vec()
        }

        fn live(&self) -> &Live {
            self.world.resource::<Particles>().systems.values().flatten().flatten().next().unwrap()
        }

        /// The geometry the draw before culling made of the live particles this frame,
        /// the camera turned by `rotation`.
        fn expected(&self, rotation: Quat) -> Vec<[f32; 3]> {
            let to_game = crate::world::game_to_bevy().compute_affine().inverse();
            let [right, up, back] = camera_basis(rotation, &to_game);
            let live = self.live();
            reference_geometry(&live.def, &live.particles, right, up, back).positions
        }
    }

    /// A visible system is drawn exactly as before culling; turned away from, it is hidden
    /// and its mesh left alone; turned back to, it is rebuilt and shown in that same frame.
    #[test]
    fn a_system_off_screen_is_hidden_and_not_rebuilt_and_back_on_screen_rebuilt_at_once() {
        for strip in [None, Some(Strip { max_points: 4, step: 0.05 })] {
            let mut s = Scene::new(moving_particles(), strip, Quat::IDENTITY);
            s.frame(0.1);
            s.frame(0.1);
            assert!(s.shown(), "in view: shown");
            assert_eq!(s.mesh_positions(), s.expected(Quat::IDENTITY), "in view: today's geometry, bit for bit");
            let before = s.mesh_positions();
            let main = s.main;
            s.turn(main, away());
            s.frame(0.1);
            assert!(!s.shown(), "out of view: hidden");
            assert_eq!(s.mesh_positions(), before, "out of view: the mesh is not rebuilt");
            s.frame(0.1);
            assert_eq!(s.mesh_positions(), before);
            // Back in view: rebuilt for this frame's particles and shown in the same frame.
            s.turn(main, Quat::IDENTITY);
            s.frame(0.1);
            assert!(s.shown(), "back in view: shown at once");
            assert_eq!(s.mesh_positions(), s.expected(Quat::IDENTITY), "back in view: this frame's geometry");
            assert_ne!(s.mesh_positions(), before);
        }
    }

    /// The water reflection camera seeing a system keeps it drawn while the main camera
    /// faces away; switched off, the system is hidden. A camera of other layers does not
    /// count.
    #[test]
    fn reflection_only_visibility_keeps_a_system_drawn() {
        let mut s = Scene::new(moving_particles(), None, away());
        let reflection = Scene::camera(&mut s.world, Quat::IDENTITY, true, Some(RenderLayers::from_layers(&[0, crate::sky::ARMS_LAYER, crate::sky::BODY_LAYER])), Role::Reflection);
        s.frame(0.1);
        assert!(s.shown(), "the reflection sees it");
        assert_eq!(s.mesh_positions(), s.expected(away()), "built with the main camera's basis, as before");
        s.world.get_mut::<Camera>(reflection).unwrap().is_active = false;
        s.frame(0.1);
        assert!(!s.shown(), "reflection off: nothing sees it");
        // A camera facing it that draws other layers only (a menu view's).
        Scene::camera(&mut s.world, Quat::IDENTITY, true, Some(RenderLayers::layer(7)), Role::Other);
        s.frame(0.1);
        assert!(!s.shown(), "a camera of other layers does not count");
    }

    /// In doubt the system is drawn: no active camera, a camera whose frustum is not set.
    /// Without the world camera nothing is drawn, shown or hidden.
    #[test]
    fn in_doubt_a_system_is_drawn_and_without_the_world_camera_left_as_it_was() {
        let mut s = Scene::new(moving_particles(), None, away());
        s.frame(0.1);
        assert!(!s.shown());
        let main = s.main;
        s.world.entity_mut(main).insert(Frustum::default());
        s.frame(0.1);
        assert!(s.shown(), "a frustum not set: drawn");
        assert_eq!(s.mesh_positions(), s.expected(away()));
        // The main camera's frustum set again, and a camera of other layers only (a menu
        // view's) whose frustum is not set: that one does not stop the culling.
        s.turn(main, away());
        let menu = Scene::camera(&mut s.world, Quat::IDENTITY, true, Some(RenderLayers::layer(7)), Role::Other);
        s.world.entity_mut(menu).insert(Frustum::default());
        s.frame(0.1);
        assert!(!s.shown(), "an unset camera of other layers leaves it culled");
        // One drawing layer 0 whose frustum is not set: drawn.
        s.world.entity_mut(menu).insert(RenderLayers::from_layers(&[0, 7]));
        s.frame(0.1);
        assert!(s.shown(), "an unset camera of its layers: drawn");
        s.world.entity_mut(menu).despawn();
        s.world.get_mut::<Camera>(main).unwrap().is_active = false;
        s.turn(main, away());
        s.frame(0.1);
        assert!(s.shown(), "no active camera: drawn");
        // No world camera (none spawned yet): the draw stops, leaving the system as it was.
        s.world.get_mut::<Camera>(main).unwrap().is_active = true;
        s.world.entity_mut(main).remove::<crate::fly::FlyCam>();
        let before = s.mesh_positions();
        s.frame(0.1);
        assert!(s.shown(), "no world camera: left shown");
        assert_eq!(s.mesh_positions(), before, "no world camera: not rebuilt");
    }

    /// A scope raised beside the world camera (a 2D scope's camera on its own layer, a 3D
    /// scope's lens camera on the world's) no longer stops the draw: a system culled when
    /// the scope comes up is rebuilt and shown in the frame the player turns to it.
    #[test]
    fn a_system_culled_when_a_scope_is_raised_comes_back_when_turned_to() {
        for lens in [false, true] {
            let mut s = Scene::new(moving_particles(), None, away());
            s.frame(0.1);
            assert!(!s.shown(), "out of view: culled");
            let before = s.mesh_positions();
            // The scope comes up: its camera beside the world camera.
            let scope = if lens { Scene::camera(&mut s.world, away(), true, Some(RenderLayers::from_layers(&[0, crate::reflection::WATER_LAYER])), Role::Other) } else { Scene::camera(&mut s.world, Quat::IDENTITY, true, Some(RenderLayers::layer(7)), Role::Other) };
            s.frame(0.1);
            assert!(!s.shown(), "scope up, still out of view: culled (lens {lens})");
            assert_eq!(s.mesh_positions(), before);
            // The player turns to the system with the scope up (the lens camera turns with
            // the world camera it hangs from).
            let main = s.main;
            s.turn(main, Quat::IDENTITY);
            if lens {
                s.turn(scope, Quat::IDENTITY);
            }
            s.frame(0.1);
            assert!(s.shown(), "turned to with the scope up: shown at once (lens {lens})");
            assert_eq!(s.mesh_positions(), s.expected(Quat::IDENTITY), "this frame's geometry (lens {lens})");
            // And it keeps animating while the scope stays up.
            let now = s.mesh_positions();
            s.frame(0.1);
            assert_ne!(s.mesh_positions(), now, "keeps animating with the scope up (lens {lens})");
        }
    }

    /// Culling changes nothing of the simulation: over many frames of an emitter giving
    /// birth all the time and particles dying, the particles, their trails, the random
    /// state and the counts are the same whether the camera sees the system or not.
    #[test]
    fn simulation_is_the_same_with_the_system_culled_or_drawn() {
        for strip in [None, Some(Strip { max_points: 5, step: 0.03 })] {
            let mut seen = Scene::emitting(strip, Quat::IDENTITY);
            let mut culled = Scene::emitting(strip, away());
            let mut most = 0;
            for k in 0..150 {
                let dt = 0.004 + (k % 7) as f32 * 0.003;
                seen.frame(dt);
                culled.frame(dt);
                most = most.max(seen.live().particles.len());
            }
            assert!(seen.shown() && !culled.shown());
            let state = |s: &Scene| {
                let p = s.world.resource::<Particles>();
                let l = s.live();
                let q: Vec<_> = l.particles.iter().map(|q| (q.pos.to_array().map(f32::to_bits), q.vel.to_array().map(f32::to_bits), q.age.to_bits(), q.life.to_bits(), q.radius.to_bits(), q.angle.to_bits(), q.tex, q.trail.iter().map(|t| t.to_array().map(f32::to_bits)).collect::<Vec<_>>(), q.trail_clock.to_bits())).collect();
                (q, p.rng.0, p.born, p.most_alive, l.served, l.owed.to_bits())
            };
            assert_eq!(state(&seen), state(&culled));
            // Births and deaths both happened: far more born than ever alive at once.
            let born = seen.world.resource::<Particles>().born;
            assert!(born > 40 && (most as u64) < born, "born {born}, most alive {most}");
            assert!(!seen.live().particles.is_empty());
        }
    }

    /// With the profile aid's counts present, the draw counts each live system once a
    /// frame: rebuilt or culled.
    #[test]
    fn the_profile_counts_published_and_culled_systems() {
        let mut s = Scene::new(moving_particles(), None, Quat::IDENTITY);
        s.world.insert_resource(crate::profile::ParticleCull::default());
        s.frame(0.1);
        let main = s.main;
        s.turn(main, away());
        s.frame(0.1);
        s.frame(0.1);
        assert_eq!(s.world.resource::<crate::profile::ParticleCull>().totals(), ([1, 0], [2, 0]));
    }

    // --- The real schedule: Bevy's transform, camera, visibility and asset plugins ---

    /// The particle meshes' `Modified` events of this frame, read in `Last`.
    #[derive(Resource, Default)]
    struct ModifiedThisFrame(Vec<AssetId<Mesh>>);

    fn record_modified(mut events: MessageReader<AssetEvent<Mesh>>, mut seen: ResMut<ModifiedThisFrame>) {
        seen.0.clear();
        seen.0.extend(events.read().filter_map(|e| match e {
            AssetEvent::Modified { id } => Some(*id),
            _ => None,
        }));
    }

    /// A reflection camera to switch on this frame in `PostUpdate`, where
    /// `reflection::update_reflection` does: after the transforms, before the frusta.
    #[derive(Resource, Default)]
    struct RaiseReflection(Option<Entity>);

    fn raise_reflection(mut raise: ResMut<RaiseReflection>, mut cameras: Query<(&mut Camera, &mut Transform, &mut GlobalTransform)>) {
        let Some(e) = raise.0.take() else { return };
        if let Ok((mut c, mut t, mut g)) = cameras.get_mut(e) {
            c.is_active = true;
            *t = Transform::IDENTITY;
            *g = GlobalTransform::from(*t);
        }
    }

    /// A headless app with Bevy's own plugins and sets (no window, no GPU) and the
    /// particle systems where the plugin puts them: `simulate` and `take_basis` in
    /// `Update`, `draw` in `ParticleDrawSet` (`configure_draw_set`). One system ahead of
    /// the world camera's identity turn, under the world root; the world camera faces away.
    struct Sched {
        app: App,
        system: Entity,
        mesh: Handle<Mesh>,
        main: Entity,
        reflection: Entity,
    }

    impl Sched {
        fn new() -> Sched {
            use bevy::asset::{AssetApp, AssetPlugin};
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::transform::TransformPlugin, bevy::camera::CameraPlugin)).init_asset::<Mesh>()
                // CameraPlugin's skinned-mesh bounds system reads this store (MeshPlugin adds it in the game).
                .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();
            configure_draw_set(&mut app);
            app.init_resource::<ModifiedThisFrame>()
                .init_resource::<RaiseReflection>()
                .add_systems(Update, (simulate, take_basis).chain())
                .add_systems(
                    PostUpdate,
                    (
                        draw.in_set(ParticleDrawSet),
                        raise_reflection.after(bevy::transform::TransformSystems::Propagate).before(VisibilitySystems::UpdateFrusta),
                    ),
                )
                .add_systems(Last, record_modified);
            let world = app.world_mut();
            let root = world.spawn((WorldRoot, crate::world::game_to_bevy(), Visibility::default())).id();
            let main = world.spawn((Camera3d::default(), Camera::default(), test_projection(), Transform::from_rotation(away()), crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 })).id();
            let reflection = world
                .spawn((
                    Camera3d::default(),
                    Camera { is_active: false, ..default() },
                    test_projection(),
                    Transform::from_rotation(away()),
                    crate::reflection::ReflectionCamera,
                    RenderLayers::from_layers(&[0, crate::sky::ARMS_LAYER, crate::sky::BODY_LAYER]),
                ))
                .id();
            let mesh = world.resource_mut::<Assets<Mesh>>().add(particle_mesh());
            let system = world.spawn((Mesh3d(mesh.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, NoFrustumCulling, ChildOf(root))).id();
            let mut p = Particles::default();
            let live = Live { def: test_def(None), particles: moving_particles(), geometry: ParticleGeometry::default(), mesh: mesh.clone(), entity: system, served: 0, owed: 0.0, camera_local: false };
            p.systems.insert(Key { source: Source::Model("test.nif".into()), instance: None }, Some(vec![live]));
            world.insert_resource(p);
            let mut s = Sched { app, system, mesh, main, reflection };
            for _ in 0..3 {
                s.app.update();
            }
            s
        }

        /// Run a frame: whether the system is drawn (inherited and view visibility, as the
        /// renderer would take it), and whether its mesh's `Modified` event came this frame.
        fn frame(&mut self) -> (bool, bool, bool) {
            self.app.update();
            let w = self.app.world();
            let inherited = w.get::<InheritedVisibility>(self.system).unwrap().get();
            let view = w.get::<ViewVisibility>(self.system).unwrap().get();
            (inherited, view, w.resource::<ModifiedThisFrame>().0.contains(&self.mesh.id()))
        }

        fn culled(&mut self, what: &str) {
            assert_eq!(self.frame(), (false, false, false), "{what}: out of view, hidden, mesh not rewritten");
        }

        fn entered(&mut self, what: &str) {
            assert_eq!(self.frame(), (true, true, true), "{what}: drawn and its mesh rewritten in the frame it comes into view");
        }

        fn set<B: Bundle>(&mut self, e: Entity, b: B) {
            self.app.world_mut().entity_mut(e).insert(b);
        }
    }

    /// In the real schedule a system coming into view is shown, seen by the view and its
    /// rebuilt mesh sent to the renderer in that same frame: the camera turned, moved, its
    /// field of view widened, the water reflection camera switched on that frame, and
    /// turned to with a scope's camera up.
    #[test]
    fn real_schedule_draws_a_system_in_the_frame_it_comes_into_view() {
        // Turned to.
        let mut s = Sched::new();
        s.culled("facing away");
        s.culled("facing away");
        let main = s.main;
        s.set(main, Transform::IDENTITY);
        s.entered("turned to");
        assert_eq!(s.frame(), (true, true, true), "in view: rebuilt every frame as before");
        s.set(main, Transform::from_rotation(away()));
        s.culled("turned away again");

        // Moved: from past the system (it behind the camera) to in front of it.
        let mut s = Sched::new();
        let main = s.main;
        s.set(main, Transform::from_xyz(0.0, 0.0, -2000.0));
        s.culled("past it");
        s.set(main, Transform::IDENTITY);
        s.entered("moved in front of it");

        // A wider field of view: turned 40° off it, outside a 10° view, inside a 100° one.
        let mut s = Sched::new();
        let main = s.main;
        let narrow = Projection::Perspective(PerspectiveProjection { fov: 10f32.to_radians(), aspect_ratio: 16.0 / 9.0, near: 5.0, ..default() });
        let wide = Projection::Perspective(PerspectiveProjection { fov: 100f32.to_radians(), aspect_ratio: 16.0 / 9.0, near: 5.0, ..default() });
        s.set(main, (Transform::from_rotation(Quat::from_rotation_y(40f32.to_radians())), narrow));
        s.culled("off to the side of a narrow view");
        s.set(main, wide);
        s.entered("the view widened");

        // The reflection camera switched on this frame (in PostUpdate, before the frusta).
        let mut s = Sched::new();
        s.culled("no camera faces it");
        let reflection = s.reflection;
        s.app.world_mut().resource_mut::<RaiseReflection>().0 = Some(reflection);
        s.entered("the reflection camera switched on facing it");
        s.app.world_mut().get_mut::<Camera>(reflection).unwrap().is_active = false;
        s.culled("the reflection camera off again");

        // A scope's camera up (its own layer), then turned to.
        let mut s = Sched::new();
        s.culled("facing away");
        s.app.world_mut().spawn((Camera3d::default(), Camera::default(), test_projection(), Transform::IDENTITY, RenderLayers::layer(7)));
        s.culled("a scope's camera up");
        let main = s.main;
        s.set(main, Transform::IDENTITY);
        s.entered("turned to with the scope up");
    }
}
