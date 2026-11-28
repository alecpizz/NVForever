//! Prepared static models: a NIF parsed once into a flat list of (mesh, material,
//! transform) so that thousands of references to the same model share GPU meshes and
//! materials (Bevy batches identical mesh/material pairs).
//!
//! Shapes whose object or ancestors are hidden (flag bit 0), editor markers and helper
//! geometry (see `Props::is_helper`) are dropped. Skinned shapes are drawn in their bind
//! pose: their vertices are stored in the shape's own space (docs/spec-notes/nif.md §6).
//!
//! Models with an embedded `Idle` (or, failing that, `SpecialIdle`) sequence also get a
//! [`Rig`]: their node hierarchy, spawned per reference so the sequence can move nodes
//! and skinned shapes (waving flags, windmills).

use crate::anim_play::{Clip, MaterialTarget, MaterialTrack, OwnCycle, Playing};
use crate::material_anim::{MaterialSlot, RigMaterials};
use crate::data::GameData;
use crate::nif_material::NifMaterial;
use crate::nif_spawn::{affine, is_editor_marker, shaded_model, shape_material_with, shape_props, to_bevy, AdmittedShape, NumericRejections, StoreTextures, TextureSource};
use crate::texture::{worker_texture, worker_texture_exact, TextureCache, TextureSnapshot, TextureUsage, WorkerTextures};
use bevy::asset::AssetHandleProvider;
use bevy::image::CompressedImageFormats;
use bevy::tasks::{futures::check_ready, Task};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::math::Affine3A;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use fnv_nif::anim::{geom_morphers, sequences, transform_track, value_track, BoolTrack, Channel, GeomMorpher, TransformTrack, ValueTrack};
use fnv_nif::scene::{scene_objects, segment_triangles, Decoded, SceneObject};
use fnv_nif::{NifFile, Struct};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub use fnv_world::alt_textures::AltTexture;
mod attachment;
pub mod geometry;
pub use attachment::ModelObject;

#[derive(Debug, Clone)]
pub struct PreparedShape {
    pub mesh: Handle<Mesh>,
    pub material: Handle<NifMaterial>,
    /// Shape transform in NIF (game) space relative to the root node's own frame: the
    /// root's transform is not included (see [`PreparedModel::root`]).
    pub transform: Affine3A,
    pub name: Option<String>,
    /// Names of the nodes above the shape, nearest first, the root excluded (parts of
    /// a weapon model are shown, hidden and attached by them: animation.md §9.7).
    pub ancestors: Vec<String>,
    /// The node the shape is under ([`PreparedModel::nodes`]; `None`: the root) and its
    /// own transform below that node. `transform` is the nodes' transforms from the root
    /// down times this.
    pub node: Option<usize>,
    pub own: Affine3A,
    /// CPU copy of the mesh, kept only for models loaded with
    /// [`ModelCtx::model_with_cpu`]. GPU meshes are render-world only: once uploaded,
    /// their vertex data can no longer be read from the main world.
    pub cpu: Option<Arc<Mesh>>,
    /// The triangles of each segment of a BSSegmentedTriShape, as ranges into the mesh's
    /// triangle list (`fnv_nif::scene::segment_triangles`); empty for other shapes. Object
    /// LOD blocks group their triangles by cell this way (docs/spec-notes/world.md §7).
    pub segments: Vec<std::ops::Range<usize>>,
    /// The shader property sets BSShaderFlags bit 29 "External_Emittance": a reference
    /// that names an emittance source (REFR XEMI) drives its colour.
    pub external_emittance: bool,
    /// Its material's stored glow and alpha, for clips that drive them (a held weapon's
    /// bulbs, `material_anim`).
    pub look: crate::material_anim::Look,
    /// The diffuse texture the shape names when it could not be loaded (as the NIF names
    /// it), so its material has no base texture: nvforever's distant LOD logs it (#3096).
    pub missing_diffuse: Option<String>,
    /// The number of its block in the file, the same in every variant of the model
    /// ([`geometry`]).
    pub block: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PreparedModel {
    /// Source graph for `model_for_attachment`, from that same cached decode.
    /// Ordinary/streamed models leave this empty.
    pub objects: Vec<ModelObject>,
    pub shapes: Vec<PreparedShape>,
    /// The root node's own transform. A placed reference's transform replaces it, so
    /// world references draw shapes at `reference ∘ shape.transform`; geometry drawn
    /// without a reference (LOD blocks) uses `root ∘ shape.transform` [observed:
    /// docs/spec-notes/world.md §4].
    pub root: Affine3A,
    /// Model-space bounds in NIF (game) space, root transform included.
    pub bounds: Option<(Vec3, Vec3)>,
    pub triangles: usize,
    /// Skinned shapes among `shapes` (drawn in their bind pose).
    pub skinned: usize,
    pub rig: Option<Rig>,
    /// The nodes below the root, parents before children: for callers that pose the
    /// model's nodes themselves (a weapon's moving parts, animation.md §9.8).
    pub nodes: Vec<ModelNode>,
    /// The file holds particle systems (`NiParticleSystem`, `BSStripParticleSystem`),
    /// read by `ModelCtx::particles` (a placed fire's flames).
    pub particles: bool,
    /// The first named point or spot light of the file, when a sequence of the rig
    /// drives it (docs/spec-notes/model-lights.md): where it hangs in the rig, so that the
    /// light of a LIGH reference can follow its model (nvforever `lights.rs`).
    pub light: Option<ModelLight>,
    /// The file joins Havok bodies with constraints (a swinging trap's log and chains,
    /// a hanging lamp): its rig is built though no sequence moves it, so that the bodies
    /// can move its nodes (nvforever `swing_traps.rs`).
    pub jointed: bool,
    /// The file has two or more dynamic Havok bodies and no constraints: loose parts, a
    /// destroyed car's doors, hood and trunk (`vehicles\carhulk01.nif`) modelled where
    /// the blast throws them. Its rig is built too, so that the bodies can move its nodes
    /// once they fall (nvforever `wrecks.rs`).
    pub loose: bool,
}

/// A light inside a model whose colour or dimmer a sequence drives. Lights are not
/// scene objects (not rig nodes); the light hangs from its parent node.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelLight {
    /// The light's own name: the node name its sequences' tracks give (`object0`).
    pub name: String,
    /// The rig node listing it as a child (`None`: no node does).
    pub parent: Option<String>,
    /// Its transform under that node.
    pub local: Transform,
    /// Its stored diffuse colour and dimmer (nif.xml NiLight) [documented].
    pub diffuse: [f32; 3],
    pub dimmer: f32,
}

/// A node of a model below its root.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelNode {
    pub name: Option<String>,
    /// Index in [`PreparedModel::nodes`]; `None`: the root.
    pub parent: Option<usize>,
    /// Local transform in NIF (game) space.
    pub local: Affine3A,
    /// A `NiBillboardNode`: its billboard mode [documented: nif.xml BillboardMode].
    pub billboard: Option<u16>,
    /// A `BSValueNode`: its value, the node index of the ADDN whose particle effect it
    /// stands for [observed: the value is the number in the node's name `AddOnNode<n>`
    /// in every file looked at; docs/spec-notes/particles.md].
    pub addon: Option<i32>,
    /// A `BSDamageStage`: the model damage stages, lowest and highest, its children are
    /// shown at (its `min` and `max`, from the name `DamageStage<min>-<max>`)
    /// [documented: nif.xml BSRangeNode fields; hypothesis: their meaning,
    /// docs/spec-notes/destruction-stages.md].
    pub damage_stages: Option<(i32, i32)>,
}

/// How many of a file's Havok bodies are dynamic: motion systems 1 to 5, 8 and 9, as
/// `fnv_collide::nif::motion` reads them [documented: nif.xml hkMotionType].
fn dynamic_bodies(file: &NifFile, dec: &Decoded) -> usize {
    let motion = |i: usize| dec.blocks[i].as_ref().ok().and_then(|b| b.strukt("rigid_body_info")).and_then(|s| s.int("motion_system"));
    file.blocks.iter().enumerate().filter(|(i, b)| b.type_name.starts_with("bhkRigidBody") && is_dynamic_motion(motion(*i))).count()
}

/// Whether a body's stored motion system moves it under physics.
fn is_dynamic_motion(system: Option<i64>) -> bool {
    matches!(system, Some(1..=5 | 8 | 9))
}

/// The model damage stages node `i` of `nodes` is shown at: the overlap of the ranges of
/// the `BSDamageStage` nodes it is, or is below; `None`: at every stage.
pub fn damage_stages_of(nodes: &[ModelNode], i: usize) -> Option<(i32, i32)> {
    let mut out: Option<(i32, i32)> = None;
    let mut at = Some(i);
    while let Some(n) = at {
        if let Some((lo, hi)) = nodes[n].damage_stages {
            out = Some(out.map_or((lo, hi), |(a, b)| (a.max(lo), b.min(hi))));
        }
        at = nodes[n].parent;
    }
    out
}

/// A `NiBillboardNode` drawn as an entity of its own: its billboard mode and its rotation
/// in the file. nvforever turns it towards the camera each frame (`world::face_camera`,
/// docs/spec-notes/billboard-nodes.md): the glow cards of placed torches and fires, and
/// those of the effect models.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Billboard {
    pub mode: u16,
    pub bind: Quat,
}

/// The nearest billboard node at or above node `n` of `nodes` (`None`: under none, or
/// the root itself).
pub fn billboard_above(nodes: &[ModelNode], n: Option<usize>) -> Option<usize> {
    let mut at = n;
    while let Some(i) = at {
        if nodes[i].billboard.is_some() {
            return Some(i);
        }
        at = nodes[i].parent;
    }
    None
}

/// Node `n`'s transform from the model's root (the root's own excluded, as in
/// [`PreparedShape::transform`]); `None`: the root, the identity.
pub fn node_from_root(nodes: &[ModelNode], n: Option<usize>) -> Affine3A {
    let mut m = Affine3A::IDENTITY;
    let mut at = n;
    while let Some(i) = at {
        m = nodes[i].local * m;
        at = nodes[i].parent;
    }
    m
}

/// The billboard nodes of a model drawn with its shapes baked (a placed model with no
/// node hierarchy of its own: render streaming's static path, a reference enabled by
/// script): each shape below a billboard node goes under an entity for that node, which
/// carries [`Billboard`] and so can turn, instead of being baked in the file's pose
/// (#2549). One per reference; a node's entity is made when a shape first needs it. A
/// billboard node below another is turned alone, the outer one staying baked.
#[derive(Default)]
pub struct BakedBillboards {
    made: HashMap<usize, Entity>,
}

impl BakedBillboards {
    /// Where shape `s` of `model`, placed by `rt` under `parent`, goes: `None` when it is
    /// under no billboard node (baked as before, `rt ∘ s.transform`); else the billboard
    /// node's entity (made the first time, under a holder at `rt ∘` the node's parent from
    /// the root) and the shape's transform below it. `tag` goes on the holder when it is
    /// made: render streaming's static path gives it the reference's `RefVisual`, so that
    /// what moves or redraws a reference by its drawn children of the cell (nvf-play
    /// moved.rs `follow`, destroyed.rs `swap_models`) takes the billboard's shapes along.
    pub fn place<B: Bundle>(&mut self, commands: &mut Commands, parent: Entity, model: &PreparedModel, rt: Affine3A, s: &PreparedShape, tag: B) -> Option<(Entity, Affine3A)> {
        let b = billboard_above(&model.nodes, s.node)?;
        let node = &model.nodes[b];
        let e = *self.made.entry(b).or_insert_with(|| {
            let above = rt * node_from_root(&model.nodes, node.parent);
            let holder = commands.spawn((Transform::from_matrix(Mat4::from(above)), Visibility::Inherited, tag, ChildOf(parent))).id();
            let t = Transform::from_matrix(Mat4::from(node.local));
            commands.spawn((t, Visibility::Inherited, Billboard { mode: node.billboard.unwrap_or(0), bind: t.rotation }, ChildOf(holder))).id()
        });
        Some((e, node_from_root(&model.nodes, Some(b)).inverse() * s.transform))
    }

    /// How many billboard nodes were made.
    pub fn len(&self) -> usize {
        self.made.len()
    }

    pub fn is_empty(&self) -> bool {
        self.made.is_empty()
    }
}

/// Where a light model's light sits, as a transform from the model's root: its node
/// named `AttachLight` (or `AttachLight<n>`), with the nodes above it
/// (docs/spec-notes/model-lights.md §4). `None`: no such node.
pub fn attach_light(nodes: &[ModelNode]) -> Option<Affine3A> {
    let is_attach = |n: &ModelNode| {
        n.name.as_deref().is_some_and(|s| s.len() >= 11 && s[..11].eq_ignore_ascii_case("AttachLight") && s[11..].bytes().all(|b| b.is_ascii_digit()))
    };
    let mut at = Some(nodes.iter().position(is_attach)?);
    let mut t = Affine3A::IDENTITY;
    while let Some(n) = at {
        t = nodes[n].local * t;
        at = nodes[n].parent;
    }
    Some(t)
}

/// The nodes of a file below its roots, parents before children, and each one's index
/// by block.
pub fn model_nodes(objects: &[SceneObject]) -> (Vec<ModelNode>, HashMap<usize, usize>) {
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let mut nodes = Vec::new();
    let mut index: HashMap<usize, usize> = HashMap::new();
    let mut queue: Vec<usize> = objects.iter().filter(|o| o.is_node && o.parent.is_none()).map(|o| o.block).collect();
    let mut i = 0;
    while i < queue.len() {
        let b = queue[i];
        i += 1;
        for &c in &by_block[&b].children {
            // An object listed by two nodes belongs to the first (`scene_objects`).
            let Some(co) = by_block.get(&c).filter(|co| co.is_node && co.parent == Some(b)) else { continue };
            if index.contains_key(&c) {
                continue;
            }
            index.insert(c, nodes.len());
            // The mode is the caller's to fill in (`ModelCtx::load`): it is not among
            // a scene object's fields.
            let billboard = (co.type_name == "NiBillboardNode").then_some(0);
            let addon = (co.type_name == "BSValueNode").then_some(-1);
            let damage_stages = (co.type_name == "BSDamageStage").then_some((0, i32::MAX));
            nodes.push(ModelNode { name: co.name.clone(), parent: index.get(&b).copied(), local: affine(&co.transform), billboard, addon, damage_stages });
            queue.push(c);
        }
    }
    (nodes, index)
}

/// The way down from the root to the first node with one of `names` (letter case
/// ignored): indices into `nodes`, the named node last.
pub fn node_path(nodes: &[ModelNode], names: &[&str]) -> Option<Vec<usize>> {
    let mut at = nodes.iter().position(|n| n.name.as_deref().is_some_and(|x| names.iter().any(|name| x.eq_ignore_ascii_case(name))));
    let mut path = Vec::new();
    while let Some(i) = at {
        path.push(i);
        at = nodes[i].parent;
    }
    path.reverse();
    (!path.is_empty()).then_some(path)
}

/// A node of a [`Rig`]; node 0 is the model root, whose own transform a placed
/// reference replaces (so it is the identity here).
#[derive(Debug, Clone)]
pub struct RigNode {
    pub name: Option<String>,
    pub parent: Option<usize>,
    /// Local transform in NIF (game) space.
    pub local: Transform,
    pub hidden: bool,
    /// A `NiBillboardNode`: its billboard mode; spawned with a [`Billboard`].
    pub billboard: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct RigShape {
    /// The node entity the mesh goes on.
    pub node: usize,
    pub mesh: Handle<Mesh>,
    pub material: Handle<NifMaterial>,
    /// Inverse bind poses and the joint nodes, for skinned shapes.
    pub skin: Option<(Handle<SkinnedMeshInverseBindposes>, Vec<usize>)>,
    /// The shape's name and its material's stored emissive colour, emissive multiplier
    /// and alpha: what a track that drives one of them leaves the others at
    /// (`crate::material_anim`).
    pub look: (String, [f32; 3], f32, f32),
    /// Its stored refraction strength (0 unless it refracts).
    pub refraction: f32,
    /// A clip of the rig drives the shape's material.
    pub animated: bool,
    /// [design, #2685] Manager morphs bound by sequence name, sampled per instance.
    pub morph: Option<Arc<crate::morph::SequenceMorph>>,
    /// As [`PreparedShape::block`].
    pub block: u32,
}

impl RigShape {
    /// The shape's material slot ([`RigMaterials`]), drawn on `entity`.
    pub fn slot(&self, entity: Entity) -> MaterialSlot {
        let (name, emissive, mult, alpha) = self.look.clone();
        let mut slot = MaterialSlot::new(name, entity, self.material.clone(), emissive, mult, alpha);
        slot.refraction = self.refraction;
        slot
    }
}

/// A door model's "Open" and "Close" sequences (docs/spec-notes/doors.md).
#[derive(Debug, Clone)]
pub struct DoorClips {
    pub open: Clip,
    pub close: Clip,
}

/// An animated model's scene graph, spawned per reference.
#[derive(Debug, Clone)]
pub struct Rig {
    pub nodes: Vec<RigNode>,
    pub shapes: Vec<RigShape>,
    /// The sequence played in a loop [hypothesis: placed objects play their embedded
    /// `Idle` sequence from load, else `SpecialIdle` (windmills); docs/spec-notes/nif.md
    /// §6].
    pub idle: Option<Clip>,
    /// A door's sequences, played when it opens or closes (nvf-play `doors`); the rig is
    /// drawn in its stored pose until then.
    pub door: Option<DoorClips>,
    /// Every named sequence that moves a node or drives a material of this file, in file
    /// order: the animation groups scripts play by name (PlayGroup, nvforever
    /// `object_anims.rs`; docs/spec-notes/activators.md §5).
    pub groups: Vec<Clip>,
    /// What the model plays once from when it is put in the world as an effect (an
    /// explosion, an impact, a muzzle flash; `nvforever::shot_fx`): its `Idle` or
    /// `SpecialIdle…` sequence with every controller of the file that no sequence drives
    /// ([`effect_clip`]). `None`: nothing in the file moves.
    pub effect: Option<Clip>,
}

impl Rig {
    /// The sequence of animation group `name` (case-insensitive).
    pub fn group(&self, name: &str) -> Option<&Clip> {
        self.groups.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Whether the model has a sequence besides those played by themselves or by doors
    /// (Idle, SpecialIdle, Open, Close): one only a script plays.
    pub fn has_script_groups(&self) -> bool {
        self.groups.iter().any(|c| !["idle", "specialidle", "open", "close"].iter().any(|n| c.name.eq_ignore_ascii_case(n)))
    }
}

impl Rig {
    /// Spawn the rig under `parent` with the root at `transform`, playing the idle clip.
    /// Returns the root entity and the entities that carry meshes.
    pub fn spawn(&self, commands: &mut Commands, parent: Entity, transform: Transform, source: &str) -> (Entity, Vec<Entity>) {
        self.spawn_at_phase(commands, parent, transform, source, 0.0)
    }

    /// Like [`Self::spawn`], with the idle clip starting at `phase` (0–1) of its cycle:
    /// for base objects flagged "Random Anim Start", so copies of one model do not move
    /// in step (docs/spec-notes/animation.md §8).
    pub fn spawn_at_phase(&self, commands: &mut Commands, parent: Entity, transform: Transform, source: &str, phase: f32) -> (Entity, Vec<Entity>) {
        let (root, drawn, _) = self.spawn_nodes(commands, parent, transform, source, phase);
        (root, drawn)
    }

    /// Like [`Self::spawn_at_phase`], also returning the node entities by name (to bind
    /// other clips later, as doors do).
    pub fn spawn_nodes(
        &self,
        commands: &mut Commands,
        parent: Entity,
        transform: Transform,
        source: &str,
        phase: f32,
    ) -> (Entity, Vec<Entity>, HashMap<String, Entity>) {
        self.spawn_playing(commands, parent, transform, source, phase, self.idle.as_ref())
    }

    /// Like [`Self::spawn_nodes`], playing `clip` instead of the idle clip (an effect
    /// model's [`Rig::effect`], `nvforever::shot_fx`).
    pub fn spawn_playing(
        &self,
        commands: &mut Commands,
        parent: Entity,
        transform: Transform,
        source: &str,
        phase: f32,
        clip: Option<&Clip>,
    ) -> (Entity, Vec<Entity>, HashMap<String, Entity>) {
        let (root, drawn, names, _) = self.spawn_indexed(commands, parent, transform, source, phase, clip);
        (root, drawn, names)
    }

    /// The existing rig producer plus its exact node vector, independent of names.
    pub fn spawn_indexed(
        &self, commands: &mut Commands, parent: Entity, transform: Transform,
        source: &str, phase: f32, clip: Option<&Clip>,
    ) -> (Entity, Vec<Entity>, HashMap<String, Entity>, Vec<Entity>) {
        let entities: Vec<Entity> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let local = if i == 0 { transform } else { n.local };
                let vis = if n.hidden { Visibility::Hidden } else { Visibility::Inherited };
                let mut e = commands.spawn((local, vis));
                if let Some(mode) = n.billboard.filter(|_| i > 0) {
                    e.insert(Billboard { mode, bind: local.rotation });
                }
                e.id()
            })
            .collect();
        commands.entity(parent).add_child(entities[0]);
        for (i, n) in self.nodes.iter().enumerate().skip(1) {
            commands.entity(entities[n.parent.unwrap_or(0)]).add_child(entities[i]);
        }
        let mut drawn = Vec::with_capacity(self.shapes.len());
        for s in &self.shapes {
            let e = entities[s.node];
            let mut cmd = commands.entity(e);
            cmd.insert((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone())));
            if let Some(morph) = &s.morph {
                cmd.insert(crate::morph::PlayingMorph::new(entities[0], morph.clone()));
            }
            if let Some((binds, joints)) = &s.skin {
                let joints = joints.iter().map(|&j| entities[j]).collect();
                cmd.insert((SkinnedMesh { inverse_bindposes: binds.clone(), joints }, DynamicSkinnedMeshBounds));
            }
            drawn.push(e);
        }
        let mut by_name: HashMap<String, Entity> = HashMap::new();
        for (n, e) in self.nodes.iter().zip(&entities) {
            if let Some(name) = &n.name {
                by_name.entry(name.clone()).or_insert(*e);
            }
        }
        if let Some(idle) = clip {
            let playing = Playing::new(source, idle, |n| by_name.get(n).copied()).at_phase(phase);
            commands.entity(entities[0]).insert(playing);
        }
        let slots: Vec<MaterialSlot> = self.shapes.iter().filter(|s| s.animated).map(|s| s.slot(entities[s.node])).collect();
        if !slots.is_empty() {
            let idle = Arc::new(clip.map(|c| c.materials.clone()).unwrap_or_default());
            commands.entity(entities[0]).insert(RigMaterials { slots, idle, dark: false, fade: 1.0 });
        }
        (entities[0], drawn, by_name, entities)
    }
}

/// The material tracks of the controllers in a file that are not driven by a sequence:
/// `NiTextureTransformController`s on a shape's `NiTexturingProperty` that move, scale or
/// turn its base texture, and `NiAlphaController`s, `NiMaterialColorController`s of the
/// emissive colour and `BSMaterialEmittanceMultController`s on its `NiMaterialProperty`
/// (glowing consoles, laser tripwires, effects: docs/spec-notes/effects-animation-audit.md
/// §3). Each keeps its own timing ([`OwnCycle`]). Effect meshes are built so: the smoke wisps
/// (`effects/ambient/fxsmokewisps01.nif`: V in 36.67 s, U in 44.1 s, both looping) and
/// the ground smoke at camp fires (`dlcpitt/effects/dlcpittfxgroundsmoke01.nif`: V in
/// 13.3 s) [observed: `fnvtool nif decode`; docs/spec-notes/nif.md §6 "Controllers
/// outside sequences"].
///
/// Taken: controllers with the Active flag (bit 3) and without the "manager controlled"
/// flag (bit 5), whose interpolator gives float values [documented: nif.xml
/// NiTimeController flags]; a controller a sequence drives is played with that sequence.
pub fn free_material_tracks(file: &NifFile, dec: &Decoded) -> Vec<MaterialTrack> {
    material_tracks_of_own(file, dec, false)
}

/// For each named shape, the other named shapes that list the same `NiMaterialProperty`
/// block (lower-case names). A sequence's material track names one shape, but its
/// controller animates the property block, which every shape listing it wears
/// [documented: nif.xml NiTimeController "Target", ControlledBlock "Node Name" /
/// "Property Type"; properties are shared by reference]: the vintage radio's Open and
/// Close name `VintageRadioBody:0`, whose material the needle and the glass dial (with the
/// glow-mapped dial face) list too [observed: `vintageradio01.nif`, docs/spec-notes/activators.md §2].
pub fn material_sharers(file: &NifFile, dec: &Decoded) -> HashMap<String, Vec<String>> {
    property_sharers(file, dec, "NiMaterialProperty")
}

/// For each named shape, the other named shapes that list the same `NiTexturingProperty`
/// block (lower-case names), as [`material_sharers`] for the material: a texture
/// transform track names one shape, but its controller moves the property's base
/// texture, which every shape listing it wears. The Hidden Valley sand storm's
/// SpecialIdle names `NVSandstorm01:0`; its 17 other dust sheets list the same
/// `NiTexturingProperty` (and `NiMaterialProperty`) [observed:
/// `effects\nv\nvsandstormnight.nif`, `fnvtool nif decode`], and only the first one
/// scrolled (#2450).
pub fn texturing_sharers(file: &NifFile, dec: &Decoded) -> HashMap<String, Vec<String>> {
    property_sharers(file, dec, "NiTexturingProperty")
}

/// For each named shape, the other named shapes listing the same property block of type
/// `kind` (lower-case names).
fn property_sharers(file: &NifFile, dec: &Decoded, kind: &str) -> HashMap<String, Vec<String>> {
    let mut users: HashMap<i32, Vec<String>> = HashMap::new();
    for b in dec.blocks.iter().flatten() {
        let Some(name) = b.text("name").filter(|n| !n.is_empty()) else { continue };
        for &r in b.refs("properties").unwrap_or_default() {
            if file.blocks.get(r as usize).is_some_and(|p| p.type_name == kind) {
                let v = users.entry(r).or_default();
                if !v.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                    v.push(name.to_ascii_lowercase());
                }
            }
        }
    }
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for names in users.values().filter(|v| v.len() > 1) {
        for n in names {
            let e = out.entry(n.clone()).or_default();
            for m in names.iter().filter(|m| *m != n) {
                if !e.contains(m) {
                    e.push(m.clone());
                }
            }
        }
    }
    out
}

/// Give the shapes that share a driven shape's property block (named as the file names
/// them in `names`) the same tracks: the members a `NiMaterialProperty` holds (emissive
/// colour and multiplier, alpha) go to the shapes wearing the same material
/// ([`material_sharers`]), the base texture's offset, scale and rotation to those
/// wearing the same `NiTexturingProperty` ([`texturing_sharers`]); refraction (on the
/// shader property) is not shared. A shape the clip already drives for a member keeps
/// its own track. Returns the shapes added.
pub fn share_material_tracks(clip: &mut Clip, material: &HashMap<String, Vec<String>>, texturing: &HashMap<String, Vec<String>>, names: &HashMap<String, String>) -> Vec<String> {
    let mut added: Vec<MaterialTrack> = Vec::new();
    for t in &clip.materials {
        let sharers = match t.target {
            MaterialTarget::SelfIllum | MaterialTarget::EmissiveMult | MaterialTarget::Alpha => material,
            MaterialTarget::UvOffset(_) | MaterialTarget::UvScale(_) | MaterialTarget::UvRotate => texturing,
            MaterialTarget::Refraction => continue,
        };
        for other in sharers.get(&t.shape.to_ascii_lowercase()).into_iter().flatten() {
            let Some(name) = names.get(other) else { continue };
            let driven = |m: &MaterialTrack| m.target == t.target && m.shape.eq_ignore_ascii_case(name);
            if clip.materials.iter().any(driven) || added.iter().any(driven) {
                continue;
            }
            added.push(MaterialTrack { shape: name.clone(), ..t.clone() });
        }
    }
    let mut shapes: Vec<String> = Vec::new();
    for t in &added {
        if !shapes.contains(&t.shape) {
            shapes.push(t.shape.clone());
        }
    }
    clip.materials.extend(added);
    shapes
}

/// The material tracks of the controllers no sequence drives ([`free_material_tracks`]);
/// `inactive` also takes those without the Active flag, for effect models
/// ([`effect_clip`]). The one reader of such controllers: placed objects play them
/// looping on their own cycles, effects once.
fn material_tracks_of_own(file: &NifFile, dec: &Decoded, inactive: bool) -> Vec<MaterialTrack> {
    // The shapes that list each property.
    let mut users: HashMap<i32, Vec<String>> = HashMap::new();
    for b in dec.blocks.iter().flatten() {
        let Some(name) = b.text("name").filter(|n| !n.is_empty()) else { continue };
        for r in b.refs("properties").unwrap_or_default() {
            users.entry(*r).or_default().push(name.to_string());
        }
    }
    let mut out = Vec::new();
    for (i, b) in file.blocks.iter().enumerate() {
        let target = match b.type_name.as_str() {
            "NiTextureTransformController" | "NiAlphaController" | "NiMaterialColorController" | "BSMaterialEmittanceMultController" | "BSRefractionStrengthController" => b.type_name.as_str(),
            _ => continue,
        };
        let Ok(c) = &dec.blocks[i] else { continue };
        let flags = c.int("flags").unwrap_or(0);
        if (!inactive && flags & 0x8 == 0) || flags & 0x20 != 0 {
            continue;
        }
        let Some(prop) = c.block_ref("target").filter(|&t| t >= 0) else { continue };
        let what = match target {
            "NiTextureTransformController" => {
                // Base texture (slot 0) of the NiTexturingProperty; operation 0
                // TT_TRANSLATE_U, 1 TT_TRANSLATE_V, 2 TT_ROTATE, 3 TT_SCALE_U, 4
                // TT_SCALE_V [documented: nif.xml TransformMember].
                if c.int("texture_slot").unwrap_or(0) != 0 || c.int("shader_map").unwrap_or(0) != 0 {
                    continue;
                }
                match c.int("operation") {
                    Some(0) => MaterialTarget::UvOffset(0),
                    Some(1) => MaterialTarget::UvOffset(1),
                    Some(2) => MaterialTarget::UvRotate,
                    Some(3) => MaterialTarget::UvScale(0),
                    Some(4) => MaterialTarget::UvScale(1),
                    _ => continue,
                }
            }
            // On the shape's shader property, not its material.
            "BSRefractionStrengthController" if file.blocks.get(prop as usize).is_some_and(|p| p.type_name == "BSShaderPPLightingProperty") => MaterialTarget::Refraction,
            "BSRefractionStrengthController" => continue,
            _ if !file.blocks.get(prop as usize).is_some_and(|p| p.type_name == "NiMaterialProperty") => continue,
            "NiAlphaController" => MaterialTarget::Alpha,
            // The emissive colour (TC_SELF_ILLUM, 3) [documented: nif.xml MaterialColor];
            // the other colours are not drawn from the material here.
            "NiMaterialColorController" if c.int("target_color") == Some(3) => MaterialTarget::SelfIllum,
            "BSMaterialEmittanceMultController" => MaterialTarget::EmissiveMult,
            _ => continue,
        };
        let Some(interp) = c.block_ref("interpolator").filter(|&r| r >= 0) else { continue };
        let Some(kind) = file.blocks.get(interp as usize).map(|b| b.type_name.clone()) else { continue };
        let Some(values) = value_track(dec, interp, &kind) else { continue };
        // A rotation or scale held at no change (0, 1) all along: the screens' and many
        // effects' controllers that only restate the identity [observed:
        // `nvsecuritron*screen01.nif`, effects-animation-audit.md §2]; not worth a rig.
        let identity = match what {
            MaterialTarget::UvRotate => Some(0.0),
            MaterialTarget::UvScale(_) => Some(1.0),
            _ => None,
        };
        if identity.is_some() && constant_value(&values) == identity {
            continue;
        }
        let cycle = own_cycle(c, flags);
        if cycle.stop <= cycle.start {
            continue;
        }
        let values = Arc::new(values);
        for shape in users.get(&prop).into_iter().flatten() {
            out.push(MaterialTrack { shape: shape.clone(), target: what, values: values.clone(), cycle: Some(cycle), priority: 0 });
        }
    }
    out
}

/// The vertex morphs a model plays by shape block (docs/spec-notes/vertex-morphs.md §3):
/// `NiGeomMorpherController`s that run by themselves (Active, not manager controlled),
/// whose shape moves, with relative targets whose morph 0 has the shape's vertex count.
/// A clamped one whose
/// weights end where they start is played looping: the NCR, Nevada and Followers flags
/// (clamped, -2 s to 22.8 or 34.7 s, every weight back at its start value at the stop
/// time [observed: `examples/morph_survey.rs`]) are cycles meant to repeat
/// [hypothesis]; other clamped ones are left out [hypothesis: they play once when
/// something starts them (the tripwire's snap, the dart gun's band, an explosion's
/// splash); on a world object's clock they would have ended before anyone sees them].
fn shape_morphs(file: &NifFile, dec: &Decoded) -> HashMap<usize, GeomMorpher> {
    let mut out = HashMap::new();
    for mut g in geom_morphers(file, dec) {
        let Ok(target) = usize::try_from(g.target) else { continue };
        if !g.is_free() {
            continue;
        }
        let n = g.num_vertices;
        if (g.flags >> 1) & 3 == 2 {
            let seam = (1..g.morphs.len()).map(|i| (g.weight_at(i, g.start_time) - g.weight_at(i, g.stop_time)).abs()).fold(0.0, f32::max);
            if seam > 0.01 {
                continue;
            }
            g.flags &= !0x6;
        }
        if !g.relative || g.morphs.len() < 2 || g.morphs.iter().any(|m| m.vectors.len() != n) || !g.moves() {
            continue;
        }
        out.insert(target, g);
    }
    out
}

/// The first point or spot light of a file that has a name, with the node that lists it
/// as a child (the lowest-index one, as `scene_objects` takes parents).
fn model_light(file: &NifFile, dec: &Decoded) -> Option<ModelLight> {
    let (i, s) = file.blocks.iter().enumerate().filter(|(_, b)| matches!(b.type_name.as_str(), "NiPointLight" | "NiSpotLight")).find_map(|(i, _)| {
        let s = dec.blocks[i].as_ref().ok()?;
        s.text("name").filter(|n| !n.is_empty()).map(|_| (i, s))
    })?;
    let parent = dec.blocks.iter().find_map(|b| {
        let b = b.as_ref().ok()?;
        b.refs("children")?.contains(&(i as i32)).then(|| b.text("name").map(str::to_string)).flatten()
    });
    let diffuse = s.floats("diffuse_color").filter(|c| c.len() >= 3).map_or([0.0; 3], |c| [c[0], c[1], c[2]]);
    Some(ModelLight {
        name: s.text("name")?.to_string(),
        parent,
        local: to_bevy(&fnv_nif::scene::Transform::from_struct(s).unwrap_or(fnv_nif::scene::Transform::IDENTITY)),
        diffuse,
        dimmer: s.float("dimmer").unwrap_or(1.0),
    })
}

/// `clip` when it can be played ([`Clip::check`]); a broken one is logged and dropped
/// (once per model: prepared models are cached).
fn playable(path: &str, clip: Clip) -> Option<Clip> {
    match clip.check() {
        Ok(()) => Some(clip),
        Err(e) => {
            warn!("{path}: clip {} not played: {e}", clip.name);
            None
        }
    }
}

/// A controller's own timing (fields of NiTimeController [documented: nif.xml]).
fn own_cycle(c: &Struct, flags: i64) -> OwnCycle {
    OwnCycle {
        start: c.float("start_time").unwrap_or(0.0),
        stop: c.float("stop_time").unwrap_or(0.0),
        frequency: c.float("frequency").filter(|f| *f > 0.0).unwrap_or(1.0),
        phase: c.float("phase").unwrap_or(0.0),
        cycle_type: (flags >> 1) & 3,
    }
}

/// What a node controller no sequence drives plays.
enum NodeTrack {
    Transform(Arc<TransformTrack>),
    Visibility(BoolTrack),
}

/// The `NiTransformController`s and `NiVisController`s on the nodes of a file that no
/// sequence drives (not manager controlled [documented: nif.xml]): node name, track, own
/// timing; `inactive` also takes those without the Active flag. The one reader of such
/// controllers, for placed objects ([`free_transform_tracks`]) and effects
/// ([`effect_clip`]).
fn node_tracks_of_own(file: &NifFile, dec: &Decoded, inactive: bool) -> Vec<(String, NodeTrack, OwnCycle)> {
    let name_of = |b: i32| dec.get(b).and_then(|n| n.text("name")).filter(|n| !n.is_empty()).map(str::to_string);
    let mut out = Vec::new();
    for (i, b) in file.blocks.iter().enumerate() {
        if !matches!(b.type_name.as_str(), "NiTransformController" | "NiVisController") {
            continue;
        }
        let Ok(c) = &dec.blocks[i] else { continue };
        let flags = c.int("flags").unwrap_or(0);
        if (!inactive && flags & 0x8 == 0) || flags & 0x20 != 0 {
            continue;
        }
        let (Some(node), Some(interp)) = (c.block_ref("target").and_then(name_of), c.block_ref("interpolator").filter(|&r| r >= 0)) else { continue };
        let Some(kind) = file.blocks.get(interp as usize).map(|b| b.type_name.as_str()) else { continue };
        let track = if b.type_name == "NiVisController" {
            let Some(t) = fnv_nif::anim::bool_track(dec, interp) else { continue };
            NodeTrack::Visibility(t)
        } else {
            let Some(t) = transform_track(dec, interp, kind) else { continue };
            NodeTrack::Transform(Arc::new(t))
        };
        out.push((node, track, own_cycle(c, flags)));
    }
    out
}

/// The transform tracks of the `NiTransformController`s in a file that are not driven by
/// a sequence and move their node: by node name, each with its own timing ([`OwnCycle`]),
/// played looping on placed objects (`Clip::free`). The vultures circling over the
/// wasteland (`effects\ambient\fxvultures01.nif`), the turning light-ray fans
/// (`effects\ambient\megatonfanltrays01.nif`), the Mothership's holograms are built so
/// [observed: `crates/fnv-nif/examples/effect_anim_survey.rs`,
/// docs/spec-notes/effects-animation-audit.md §3]. Taken as [`free_material_tracks`]
/// takes its controllers (Active, not manager controlled); a controller whose node stays
/// where it is over its span is left out (most of them only restate the node's place).
pub fn free_transform_tracks(file: &NifFile, dec: &Decoded) -> Vec<(String, Arc<TransformTrack>, OwnCycle)> {
    node_tracks_of_own(file, dec, false)
        .into_iter()
        .filter_map(|(node, t, cycle)| match t {
            NodeTrack::Transform(t) if cycle.stop > cycle.start && moves(&t, cycle.start, cycle.stop) => Some((node, t, cycle)),
            _ => None,
        })
        .collect()
}

/// The visibility tracks of the `NiVisController`s in a file that are not driven by a
/// sequence: by node name, each with its own timing ([`OwnCycle`]), played looping on
/// placed objects (`Clip::free_visibility`). Taken as [`free_transform_tracks`] takes its
/// controllers (Active, not manager controlled). The dust tube placed near Goodsprings
/// (`effects\ambient\fxdustmeshtube01.nif`) is built so: one such controller, looping,
/// that shows its mesh for part of a 56.7 s cycle [observed: `fnvtool nif decode`,
/// docs/spec-notes/nif.md §6 "Visibility controllers"]. A track whose value never
/// changes over its keys is kept: it holds a node the file hides shown, or the reverse.
///
/// Left out: controllers on `##` nodes, the weapon parts the holder's clips show and hide
/// (animation.md §9.7, effects-animation-audit.md §1). A placed frag mine
/// (`weapons\1handminedrop\minefrag.nif`) holds one on `##FMine` (shown from 0 s,
/// clamped) [observed]; played, it made the mine an animated rig, whose shapes lost the
/// light parts `nvf_play::mines` shows from the mine's state.
pub fn free_visibility_tracks(file: &NifFile, dec: &Decoded) -> Vec<(String, BoolTrack, OwnCycle)> {
    node_tracks_of_own(file, dec, false)
        .into_iter()
        .filter_map(|(node, t, cycle)| match t {
            NodeTrack::Visibility(t) if !node.starts_with("##") && (!t.keys.is_empty() || t.value.is_some()) => Some((node, t, cycle)),
            _ => None,
        })
        .collect()
}

/// Whether a transform track's pose changes between `start` and `stop` (sampled).
fn moves(t: &TransformTrack, start: f32, stop: f32) -> bool {
    let at = |x: f32| (t.translation.sample(x), t.rotation_at(x), t.scale.sample(x));
    let first = at(start);
    (1..=32).any(|k| {
        let x = at(start + (stop - start) * k as f32 / 32.0);
        let moved = match (x.0, first.0) {
            (Some(p), Some(q)) => p.iter().zip(q).any(|(u, v)| (u - v).abs() > 1e-3),
            _ => false,
        };
        let turned = match (x.1, first.1) {
            (Some(p), Some(q)) => p.iter().zip(q).map(|(u, v)| u * v).sum::<f32>().abs() < 0.99999,
            _ => false,
        };
        let scaled = matches!((x.2, first.2), (Some([p]), Some([q])) if (p - q).abs() > 1e-3);
        moved || turned || scaled
    })
}

/// The value of a float track that never changes, if it does not.
fn constant_value(v: &ValueTrack) -> Option<f32> {
    match v {
        ValueTrack::Float(Channel::Static(Some([x]))) => Some(*x),
        ValueTrack::Float(Channel::Keys(k)) => {
            let first = k.keys.first()?.value[0];
            k.keys.iter().all(|key| key.value[0] == first && key.forward[0] == 0.0 && key.backward[0] == 0.0).then_some(first)
        }
        _ => None,
    }
}

/// What an effect model plays once from when it is put in the world
/// (docs/spec-notes/explosion-effects.md §2): `base`, its `Idle` or `SpecialIdle…`
/// sequence, if any, with every controller of the file that no sequence drives
/// (`NiTransformController`s and `NiVisController`s on its nodes, the material
/// controllers of [`material_tracks_of_own`]), whether or not it has the Active flag.
///
/// Reading [hypothesis]: impact models hold only such controllers, none with the Active
/// flag (flags 69: "APP_INIT", clamp [documented: nif.xml TimeControllerFlags]), and
/// some of their shapes store alpha 0 that only their alpha controller raises
/// (`impactexplosiondirt.nif`'s `FastMesh02`, `FastMesh03`), so the game must start
/// them when it shows the effect. Their transform and visibility keys are taken at the
/// effect's own time from 0: the keys span the controller's start and stop times, and
/// the first and last key hold outside them, which is what clamping to those times does
/// [observed: `FastMesh03` keys from 0.133 s to 0.467 s, `FastMesh01` from 0 s to
/// 0.333 s, two splashes one after the other]. A looping one of these (the muzzle
/// flashes', flags 72) plays its keys once; the flashes last 0.04 s.
pub fn effect_clip(file: &NifFile, dec: &Decoded, base: Option<Clip>) -> Option<Clip> {
    let mut tracks = Vec::new();
    let mut visibility = Vec::new();
    let mut stop = 0.0f32;
    for (node, track, cycle) in node_tracks_of_own(file, dec, true) {
        match track {
            NodeTrack::Visibility(t) => visibility.push((node, 0, t)),
            NodeTrack::Transform(t) => tracks.push((node, t)),
        }
        stop = stop.max(cycle.stop);
    }
    let materials = material_tracks_of_own(file, dec, true);
    for m in &materials {
        stop = stop.max(m.cycle.map_or(0.0, |c| c.stop));
    }
    if tracks.is_empty() && visibility.is_empty() && materials.is_empty() {
        return base;
    }
    let mut clip = base.unwrap_or_else(|| Clip { name: "Effect".into(), cycle_type: 2, stop: 0.0, ..Clip::from_controllers(Vec::new(), Vec::new(), Vec::new()) });
    clip.stop = clip.stop.max(stop);
    clip.priorities.extend(std::iter::repeat_n(0, tracks.len()));
    clip.tracks.extend(tracks);
    clip.visibility.extend(visibility);
    clip.materials.extend(materials);
    Some(clip)
}

/// A mesh with its material, ready to spawn.
pub type MeshAndMaterial = (Handle<Mesh>, Handle<NifMaterial>);

/// Prepared models by archive path; `None` remembers a failure.
#[derive(Resource, Default)]
pub struct ModelCache {
    map: HashMap<String, Option<Arc<PreparedModel>>>,
    /// The particle systems of files by archive path (`particles.rs`).
    pub(crate) particles: HashMap<String, Option<Arc<crate::particles::PreparedParticles>>>,
    /// The meshes of morphing shapes (`morph.rs`), rewritten every frame.
    pub morphs: crate::morph::MorphMeshes,
    /// The colour of the light inside files by archive path (`model_light.rs`).
    pub(crate) light_colours: HashMap<String, Option<[f32; 3]>>,
    /// VATS camera shot models by archive path (`camera_shot.rs`).
    pub(crate) camera_models: HashMap<String, Option<Arc<crate::camera_shot::CameraModel>>>,
    /// Tree billboards keyed by TREE FormID; `None` remembers a failure.
    billboards: HashMap<u32, Option<MeshAndMaterial>>,
    /// [`ModelCtx::node_strings`] by (archive path, key).
    node_strings: HashMap<(String, String), Arc<HashMap<String, String>>>,
    pub failures: Vec<String>,
    /// Models prepared on worker threads ([`ModelCtx::models_ready`]) by cache key: the
    /// work under way, and the work done that waits for [`ModelCtx::take_fetched`].
    fetching: HashMap<String, Task<Fetched>>,
    fetched: HashMap<String, Fetched>,
    /// The copy of the texture cache's list last sent to workers, with the cache's
    /// generation then; reused while a worker still holds it and the cache is unchanged.
    snapshot: Option<(u64, std::sync::Weak<TextureSnapshot>)>,
}

impl ModelCache {
    pub fn billboard(&self, id: u32) -> Option<Option<MeshAndMaterial>> {
        self.billboards.get(&id).cloned()
    }
    pub fn set_billboard(&mut self, id: u32, b: Option<MeshAndMaterial>) {
        self.billboards.insert(id, b);
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Models being prepared on worker threads or waiting to be taken in.
    pub fn in_flight(&self) -> usize {
        self.fetching.len() + self.fetched.len()
    }

    /// Prepared models held now (remembered failures not counted).
    pub fn held(&self) -> usize {
        self.map.values().filter(|m| m.is_some()).count()
    }

    /// Drop the prepared models and tree billboards nothing else holds, after their grace
    /// time (`crate::release`); the number dropped.
    pub fn release(&mut self, s: &mut crate::release::ModelSweeps, hints: &mut crate::release::ImageHints, materials: &Assets<NifMaterial>, now: f64, keep: crate::release::KeepLimits) -> usize {
        use crate::release::held_beyond;
        let entries = self.map.iter().filter_map(|(k, m)| m.as_ref().map(|m| (k.clone(), model_in_use(m))));
        let drop = s.models.sweep_timed(now, entries.collect::<Vec<_>>(), keep.secs, keep.max, |_| None);
        for (k, since) in &drop {
            if let Some(Some(m)) = self.map.remove(k) {
                let rig = m.rig.iter().flat_map(|r| &r.shapes).map(|s| &s.material);
                for h in m.shapes.iter().map(|s| &s.material).chain(rig) {
                    hints.note(materials, h, *since, now);
                }
            }
        }
        let entries = self.billboards.iter().filter_map(|(&k, b)| b.as_ref().map(|(m, t)| (k, held_beyond(m, 1) || held_beyond(t, 1))));
        let gone = s.billboards.sweep_timed(now, entries.collect::<Vec<_>>(), keep.secs, keep.max, |_| None);
        for (k, since) in &gone {
            if let Some(Some((_, m))) = self.billboards.remove(k) {
                hints.note(materials, &m, *since, now);
            }
        }
        // Particle definitions (`ModelCtx::particles`): unused when nothing holds the
        // `Arc` (a live system copies the definitions, its entity the materials) and no
        // material is held elsewhere. Remembered failures are kept.
        let entries = self.particles.iter().filter_map(|(k, p)| {
            let p = p.as_ref()?;
            let mut h = crate::release::Holders::default();
            for d in &p.systems {
                h.add(&d.material);
            }
            Some((k.clone(), Arc::strong_count(p) > 1 || h.held_elsewhere()))
        });
        let fx = s.particles.sweep_timed(now, entries.collect::<Vec<_>>(), keep.secs, keep.max, |_| None);
        for (k, since) in &fx {
            if let Some(Some(p)) = self.particles.remove(k) {
                for d in &p.systems {
                    hints.note(materials, &d.material, *since, now);
                }
            }
        }
        drop.len() + gone.len() + fx.len()
    }
}

impl ModelCache {
    /// The particle definitions held, for the stress census (`release::CacheShare`): an
    /// entry is idle when only the cache holds its `Arc` and its materials.
    pub fn particle_share(&self, materials: &Assets<NifMaterial>) -> crate::release::CacheShare {
        let mut s = crate::release::CacheShare::default();
        for p in self.particles.values().flatten() {
            s.entries += 1;
            let mut h = crate::release::Holders::default();
            for d in &p.systems {
                h.add(&d.material);
                s.images_of(materials, &d.material);
            }
            s.idle += usize::from(Arc::strong_count(p) == 1 && !h.held_elsewhere());
        }
        s
    }
}

/// Whether anything besides the cache holds a prepared model: its `Arc` (rigs, other
/// caches) or a handle of its shapes or rig (spawned entities).
fn model_in_use(m: &Arc<PreparedModel>) -> bool {
    if Arc::strong_count(m) > 1 {
        return true;
    }
    let mut h = crate::release::Holders::default();
    for s in &m.shapes {
        h.add(&s.mesh);
        h.add(&s.material);
    }
    for s in m.rig.iter().flat_map(|r| &r.shapes) {
        h.add(&s.mesh);
        h.add(&s.material);
        if let Some((b, _)) = &s.skin {
            h.add(b);
        }
    }
    h.held_elsewhere()
}

/// The asset stores [`prepare_into`] puts a model's meshes, materials and skins into.
pub(crate) trait ModelSink: TextureSource {
    fn data(&self) -> &GameData;
    fn mesh(&mut self, mesh: Mesh) -> Handle<Mesh>;
    fn material(&mut self, material: NifMaterial) -> Handle<NifMaterial>;
    fn bindposes(&mut self, binds: SkinnedMeshInverseBindposes) -> Handle<SkinnedMeshInverseBindposes>;
    /// A morphing shape's mesh ([`crate::morph::MorphMeshes`]).
    fn morph(&mut self, mesh: AssetId<Mesh>, morph: Arc<crate::morph::ShapeMorph>);
}

/// The main thread's asset stores.
pub(crate) struct Stores<'a> {
    textures: StoreTextures<'a>,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<NifMaterial>,
    bindposes: &'a mut Assets<SkinnedMeshInverseBindposes>,
    morphs: &'a mut crate::morph::MorphMeshes,
}

impl TextureSource for Stores<'_> {
    fn texture(&mut self, raw_path: &str, srgb: bool) -> Option<Handle<Image>> {
        self.textures.texture(raw_path, srgb)
    }
    fn normal(&mut self, raw_path: &str) -> Option<(Handle<Image>, bool)> {
        self.textures.normal(raw_path)
    }
    fn lacks_alpha(&self, h: &Handle<Image>) -> bool {
        self.textures.lacks_alpha(h)
    }
    fn alpha_is_mask(&self, raw_path: &str) -> bool {
        self.textures.alpha_is_mask(raw_path)
    }
    fn alpha_is_opaque(&self, raw_path: &str) -> bool {
        self.textures.alpha_is_opaque(raw_path)
    }
    fn resolve(&self, raw_path: &str) -> String {
        self.textures.resolve(raw_path)
    }
    fn pbr_set(&mut self, resolved_diffuse: &str) -> Option<crate::authored::PbrSet> {
        self.textures.pbr_set(resolved_diffuse)
    }
    fn authored_on(&self) -> bool {
        self.textures.authored_on()
    }
}

impl ModelSink for Stores<'_> {
    fn data(&self) -> &GameData {
        self.textures.data
    }
    fn mesh(&mut self, mesh: Mesh) -> Handle<Mesh> {
        self.meshes.add(mesh)
    }
    fn material(&mut self, material: NifMaterial) -> Handle<NifMaterial> {
        self.materials.add(material)
    }
    fn bindposes(&mut self, binds: SkinnedMeshInverseBindposes) -> Handle<SkinnedMeshInverseBindposes> {
        self.bindposes.add(binds)
    }
    fn morph(&mut self, mesh: AssetId<Mesh>, morph: Arc<crate::morph::ShapeMorph>) {
        self.morphs.add(mesh, morph);
    }
}

/// A worker thread's stores: handles reserved from the main thread's stores, the assets
/// kept until the main thread puts them in ([`ModelCtx::take_fetched`]). Textures come
/// from the cache's list as it was when the work was sent, or are decoded here
/// (`texture::worker_texture`).
pub(crate) struct Deferred {
    data: GameData,
    providers: Providers,
    cached: Arc<TextureSnapshot>,
    workers: Arc<WorkerTextures>,
    bc_supported: bool,
    unmasked: HashMap<AssetId<Image>, bool>,
    out: DeferredAssets,
}

/// The handle providers of the asset stores a worker prepares into.
#[derive(Clone)]
struct Providers {
    meshes: AssetHandleProvider,
    materials: AssetHandleProvider,
    bindposes: AssetHandleProvider,
    images: AssetHandleProvider,
}

impl Deferred {
    /// What it made, for [`ModelCtx::put_in`].
    pub(crate) fn into_assets(self) -> DeferredAssets {
        self.out
    }
}

/// What a worker made for one model, by the handles it reserved.
#[derive(Default)]
pub(crate) struct DeferredAssets {
    meshes: Vec<(Handle<Mesh>, Mesh)>,
    materials: Vec<(Handle<NifMaterial>, NifMaterial)>,
    bindposes: Vec<(Handle<SkinnedMeshInverseBindposes>, SkinnedMeshInverseBindposes)>,
    morphs: Vec<(AssetId<Mesh>, Arc<crate::morph::ShapeMorph>)>,
}

impl TextureSource for Deferred {
    fn texture(&mut self, raw_path: &str, srgb: bool) -> Option<Handle<Image>> {
        let usage = if srgb { TextureUsage::Color } else { TextureUsage::Linear };
        let (h, _, unmasked) = worker_texture(raw_path, usage, &self.data, &self.cached, &self.workers, &self.providers.images, self.bc_supported)?;
        self.unmasked.insert(h.id(), unmasked);
        Some(h)
    }
    fn normal(&mut self, raw_path: &str) -> Option<(Handle<Image>, bool)> {
        let (h, _, unmasked) = worker_texture(raw_path, TextureUsage::Normal, &self.data, &self.cached, &self.workers, &self.providers.images, self.bc_supported)?;
        self.unmasked.insert(h.id(), unmasked);
        Some((h, unmasked))
    }
    fn lacks_alpha(&self, h: &Handle<Image>) -> bool {
        self.unmasked.get(&h.id()).copied().unwrap_or(false)
    }
    fn alpha_is_mask(&self, raw_path: &str) -> bool {
        self.workers.alpha_is_mask(raw_path, &self.data)
    }
    fn alpha_is_opaque(&self, raw_path: &str) -> bool {
        self.workers.alpha_is_opaque(raw_path, &self.data)
    }
    fn resolve(&self, raw_path: &str) -> String {
        crate::texture::resolved_path(raw_path, |p| self.data.vfs.exists(p))
    }
    fn authored_on(&self) -> bool {
        crate::authored::enabled()
    }
    fn pbr_set(&mut self, resolved_diffuse: &str) -> Option<crate::authored::PbrSet> {
        let (data, cached, workers, images, bc, unmasked) = (&self.data, &self.cached, &self.workers, &self.providers.images, self.bc_supported, &mut self.unmasked);
        crate::authored::find(data, crate::authored::enabled(), resolved_diffuse, &workers.authored, |path, usage| {
            let (h, format, no_alpha) = worker_texture_exact(path, usage, data, cached, workers, images, bc)?;
            unmasked.insert(h.id(), no_alpha);
            Some((h, Some(format)))
        })
    }
}

impl ModelSink for Deferred {
    fn data(&self) -> &GameData {
        &self.data
    }
    fn mesh(&mut self, mesh: Mesh) -> Handle<Mesh> {
        let h = self.providers.meshes.reserve_handle().typed::<Mesh>();
        self.out.meshes.push((h.clone(), mesh));
        h
    }
    fn material(&mut self, material: NifMaterial) -> Handle<NifMaterial> {
        let h = self.providers.materials.reserve_handle().typed::<NifMaterial>();
        self.out.materials.push((h.clone(), material));
        h
    }
    fn bindposes(&mut self, binds: SkinnedMeshInverseBindposes) -> Handle<SkinnedMeshInverseBindposes> {
        let h = self.providers.bindposes.reserve_handle().typed::<SkinnedMeshInverseBindposes>();
        self.out.bindposes.push((h.clone(), binds));
        h
    }
    fn morph(&mut self, mesh: AssetId<Mesh>, morph: Arc<crate::morph::ShapeMorph>) {
        self.out.morphs.push((mesh, morph));
    }
}

/// Whether a model is landscape LOD (`meshes\landscape\lod\`: the LOD chunks and object
/// blocks nvforever's lod.rs draws), which takes no authored set (#2981).
pub(crate) fn is_lod_model(path: &str) -> bool {
    fnv_vfs::normalize(path).starts_with("meshes\\landscape\\lod\\")
}

/// A model prepared on a worker thread, waiting for the main thread.
type Fetched = (Result<PreparedModel, String>, DeferredAssets);

/// The cache key of a model: its normalised path, marked for the copy that keeps CPU
/// meshes, for the first-person copy and for a list of alternate textures (by the list's
/// hash). The CPU mark keeps a copy without CPU meshes from answering
/// [`ModelCtx::model_with_cpu`].
fn model_key(path: &str, keep_cpu: bool, first_person: bool, swaps: &[AltTexture]) -> String {
    let mut key = if first_person { format!("{path}|1stperson") } else { path.to_string() };
    if keep_cpu {
        key.push_str("|cpu");
    }
    if !swaps.is_empty() {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        swaps.hash(&mut h);
        key = format!("{key}|swap{:016x}", h.finish());
    }
    key
}

#[derive(SystemParam)]
pub struct ModelCtx<'w> {
    pub cache: ResMut<'w, ModelCache>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<NifMaterial>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub textures: ResMut<'w, TextureCache>,
    pub data: Res<'w, GameData>,
    pub formats: Option<Res<'w, CompressedImageFormatSupport>>,
    pub bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
}

impl ModelCtx<'_> {
    /// The string extra data named `key` (`UPB`, `Prn`) of the model's named nodes, by
    /// node name. Reads the file again the first time: for the few models that need it (a
    /// weapon whose parts hang from different bones); cached, as every NPC holding the
    /// weapon asks (a weapon's file read and parsed again per NPC drawn).
    pub fn node_strings(&mut self, path: &str, key: &str) -> Arc<HashMap<String, String>> {
        let k = (fnv_vfs::normalize(path), key.to_ascii_lowercase());
        if let Some(out) = self.cache.node_strings.get(&k) {
            return out.clone();
        }
        // A panic reading a broken file counts as no strings (logged once: cached).
        let read = crate::guard::guarded(|| Ok(self.read_node_strings(path, key)));
        let out = Arc::new(read.unwrap_or_else(|e| {
            warn!("{path}: {key} strings not read: {e}");
            HashMap::new()
        }));
        self.cache.node_strings.insert(k, out.clone());
        out
    }

    fn read_node_strings(&mut self, path: &str, key: &str) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let Some(bytes) = self.data.read(path) else { return out };
        let schema = self.data.schema.clone();
        let Ok(file) = NifFile::parse(&bytes, Some(&schema)) else { return out };
        let dec = Decoded::new(&file, &bytes, &schema);
        for o in scene_objects(&file, &dec).iter().filter(|o| o.is_node) {
            let (Some(name), Some(Ok(node))) = (&o.name, dec.blocks.get(o.block)) else { continue };
            for &e in node.refs("extra_data").unwrap_or_default() {
                if let Some(x) = dec.get(e).filter(|x| x.text("name").is_some_and(|n| n.eq_ignore_ascii_case(key))) {
                    out.extend(x.text("string_data").map(|s| (name.clone(), s.to_string())));
                }
            }
        }
        out
    }

    /// The prepared model at `path` (loaded on first use).
    pub fn model(&mut self, path: &str) -> Option<Arc<PreparedModel>> {
        self.load(path, false)
    }

    /// Attachment variant: capture node/property identities in the same decode as
    /// geometry. Stored in the ordinary model cache/sweeper under its own key, so
    /// ordinary world streaming does not retain an unused second scene graph.
    pub fn model_for_attachment(&mut self, path: &str) -> Option<Arc<PreparedModel>> {
        let path = fnv_vfs::normalize(path);
        let key = format!("{}|attachment", model_key(&path, false, false, &[]));
        if let Some(model) = self.cache.map.get(&key) { return model.clone(); }
        let mut stores = Stores {
            textures: StoreTextures { textures: &mut self.textures, data: &self.data, images: &mut self.images, formats: self.formats.as_deref() },
            meshes: &mut self.meshes, materials: &mut self.materials,
            bindposes: &mut self.bindposes, morphs: &mut self.cache.morphs,
        };
        let model = match crate::guard::guarded(|| prepare_unguarded(&mut stores, &path, false, false, &[], true, None)) {
            Ok(model) => Some(Arc::new(model)),
            Err(error) => { crate::guard::failed(&mut self.cache.failures, "model", format!("{key}: {error}")); None }
        };
        self.cache.map.insert(key, model.clone());
        model
    }

    /// Like [`Self::model`], also keeping a CPU copy of every shape's mesh (for code that
    /// derives new meshes from it later). Cached apart from the copy without CPU meshes.
    pub fn model_with_cpu(&mut self, path: &str) -> Option<Arc<PreparedModel>> {
        self.load(path, true)
    }

    /// [`Self::model_with_cpu`] without preparing on this thread: the model (`Some(None)`
    /// when it fails) once prepared, else `None` while a worker thread prepares it (asked
    /// for here the first time). For callers that can wait a few frames instead of
    /// reading and parsing a large file in one (`nvforever::lod` object blocks).
    pub fn model_with_cpu_ready(&mut self, path: &str) -> Option<Option<Arc<PreparedModel>>> {
        let path = fnv_vfs::normalize(path);
        let key = model_key(&path, true, false, &[]);
        if let Some(m) = self.cache.map.get(&key) {
            return Some(m.clone());
        }
        if let Some(f) = self.cache.fetched.remove(&key) {
            return Some(self.take_in(key, f));
        }
        if !self.cache.fetching.contains_key(&key) {
            self.fetch(key, path, true, Vec::new(), &mut None);
        }
        None
    }

    /// Ask a worker thread for [`Self::model_with_cpu`]'s model unless it is prepared or
    /// under way; nothing is taken in here.
    pub fn request_model_with_cpu(&mut self, path: &str) {
        let path = fnv_vfs::normalize(path);
        let key = model_key(&path, true, false, &[]);
        let c = &self.cache;
        if !(c.map.contains_key(&key) || c.fetched.contains_key(&key) || c.fetching.contains_key(&key)) {
            self.fetch(key, path, true, Vec::new(), &mut None);
        }
    }

    /// Like [`Self::model`], drawn with the first-person form of each texture that has
    /// one (`crate::texture::first_person_texture`): the weapon in the arms rig's hands.
    pub fn model_first_person(&mut self, path: &str) -> Option<Arc<PreparedModel>> {
        self.load_as(path, false, true, &[])
    }

    /// Like [`Self::model`] (or [`Self::model_first_person`]), with a record's alternate
    /// textures on the shapes they name (`fnv_world::alt_textures`; texture-swaps.md):
    /// a copy of the model of its own, cached by the list. No list: the plain model.
    pub fn model_swapped(&mut self, path: &str, first_person: bool, swaps: &[AltTexture]) -> Option<Arc<PreparedModel>> {
        self.load_as(path, false, first_person, swaps)
    }

    fn load(&mut self, path: &str, keep_cpu: bool) -> Option<Arc<PreparedModel>> {
        self.load_as(path, keep_cpu, false, &[])
    }

    fn load_as(&mut self, path: &str, keep_cpu: bool, first_person: bool, swaps: &[AltTexture]) -> Option<Arc<PreparedModel>> {
        let path = fnv_vfs::normalize(path);
        // The first-person copy is kept apart from the model's own, and so is each
        // retextured copy (by its list's hash).
        let key = model_key(&path, keep_cpu, first_person, swaps);
        if let Some(m) = self.cache.map.get(&key) {
            return m.clone();
        }
        // Prepared on a worker thread and not taken in yet. One still under way is
        // prepared here again (its work is dropped when it comes in): waiting for it could
        // mean waiting for the whole queue before it.
        if let Some(f) = self.cache.fetched.remove(&key) {
            return self.take_in(key, f);
        }
        let m = match self.prepare(&path, keep_cpu, first_person, swaps) {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                crate::guard::failed(&mut self.cache.failures, "model", format!("{key}: {e}"));
                None
            }
        };
        self.cache.map.insert(key, m.clone());
        m
    }

    /// Whether every model of `wanted` (path, alternate textures) is prepared (or known
    /// to fail), so drawing them reads no file on this thread. The rest are prepared on
    /// worker threads (read, parsed, meshes built, textures decoded), taken in by
    /// [`Self::take_fetched`]: a cell can wait for them instead of stalling the frame it
    /// is drawn in (`nvforever::world`, docs/design/streaming-hitches.md).
    pub fn models_ready<'p>(&mut self, wanted: impl IntoIterator<Item = (&'p str, &'p [AltTexture])>) -> bool {
        let mut ready = true;
        let mut shared = None;
        for (path, swaps) in wanted {
            let path = fnv_vfs::normalize(path);
            let key = model_key(&path, false, false, swaps);
            if self.cache.map.contains_key(&key) {
                continue;
            }
            ready = false;
            if self.cache.fetching.contains_key(&key) || self.cache.fetched.contains_key(&key) {
                continue;
            }
            self.fetch(key, path, false, swaps.to_vec(), &mut shared);
        }
        ready
    }

    /// Prepare the model at `path` (normalised) on a worker thread under `key`, taken in
    /// by [`Self::take_fetched`]. `shared`: the texture list and handle providers, made
    /// on the first call and reused for the rest of a batch.
    fn fetch(&mut self, key: String, path: String, keep_cpu: bool, swaps: Vec<AltTexture>, shared: &mut Option<(Arc<TextureSnapshot>, Providers)>) {
        if shared.is_none() {
            let providers = Providers {
                meshes: self.meshes.get_handle_provider(),
                materials: self.materials.get_handle_provider(),
                bindposes: self.bindposes.get_handle_provider(),
                images: self.images.get_handle_provider(),
            };
            *shared = Some((self.texture_snapshot(), providers));
        }
        let (cached, providers) = shared.clone().expect("made above");
        let mut sink = self.deferred_with(cached, providers);
        let task = crate::read_pool::read_pool().spawn(async move {
            let m = prepare_into(&mut sink, &path, keep_cpu, false, &swaps);
            (m, sink.out)
        });
        self.cache.fetching.insert(key, task);
    }

    fn deferred_with(&self, cached: Arc<TextureSnapshot>, providers: Providers) -> Deferred {
        Deferred {
            data: (*self.data).clone(),
            providers,
            cached,
            // The producer cache also owns this pool's authored DDS source metadata.
            // A snapshot hit reuses it without decoding or re-reading the DDS.
            workers: self.textures.workers.clone(),
            bc_supported: self.formats.as_deref().is_none_or(|f| f.0.contains(CompressedImageFormats::BC)),
            unmasked: HashMap::new(),
            out: DeferredAssets::default(),
        }
    }

    /// A sink for preparing on a worker thread (an actor part, `actor::ActorCtx::parts_ready`):
    /// handles reserved from these stores, textures from the cache's list or decoded there.
    /// Its assets go in with [`Self::put_in`].
    pub(crate) fn deferred(&mut self) -> Deferred {
        let providers = Providers {
            meshes: self.meshes.get_handle_provider(),
            materials: self.materials.get_handle_provider(),
            bindposes: self.bindposes.get_handle_provider(),
            images: self.images.get_handle_provider(),
        };
        let cached = self.texture_snapshot();
        self.deferred_with(cached, providers)
    }

    /// The main thread's stores as a sink (an actor part prepared here).
    pub(crate) fn stores(&mut self) -> Stores<'_> {
        Stores {
            textures: StoreTextures { textures: &mut self.textures, data: &self.data, images: &mut self.images, formats: self.formats.as_deref() },
            meshes: &mut self.meshes, materials: &mut self.materials,
            bindposes: &mut self.bindposes, morphs: &mut self.cache.morphs,
        }
    }

    /// Put what a worker's sink made into the stores: the textures every worker decoded so
    /// far, then the sink's meshes, materials, skins and morphs under their reserved handles.
    pub(crate) fn put_in(&mut self, out: DeferredAssets, now: f64) {
        self.textures.take_workers(&mut self.images, now);
        self.insert_deferred(out);
    }

    fn insert_deferred(&mut self, out: DeferredAssets) {
        for (h, mesh) in out.meshes {
            let _ = self.meshes.insert(h.id(), mesh);
        }
        for (h, material) in out.materials {
            let _ = self.materials.insert(h.id(), material);
        }
        for (h, binds) in out.bindposes {
            let _ = self.bindposes.insert(h.id(), binds);
        }
        for (id, morph) in out.morphs {
            self.cache.morphs.add(id, morph);
        }
    }

    /// The texture cache's list for workers: the last copy while it is unchanged and a
    /// worker still holds it (its strong handles keep the textures from being released
    /// only as long as workers run).
    fn texture_snapshot(&mut self) -> Arc<TextureSnapshot> {
        let generation = self.textures.generation();
        if let Some(s) = self.cache.snapshot.as_ref().filter(|(g, _)| *g == generation).and_then(|(_, w)| w.upgrade()) {
            return s;
        }
        let s = Arc::new(self.textures.snapshot(&self.images));
        self.cache.snapshot = Some((generation, Arc::downgrade(&s)));
        s
    }

    /// Put what worker threads prepared into the asset stores and the cache: every texture
    /// they decoded, and models for as long as `budget` allows (all of them without one);
    /// the number of models taken in. `now`: real seconds.
    pub fn take_fetched(&mut self, now: f64, budget: Option<std::time::Duration>) -> usize {
        let t0 = std::time::Instant::now();
        let cache = &mut *self.cache;
        cache.fetching.retain(|key, task| match check_ready(task) {
            Some(f) => {
                cache.fetched.insert(key.clone(), f);
                false
            }
            None => true,
        });
        // After the work done is collected: its textures are all in the shared list.
        self.textures.take_workers(&mut self.images, now);
        let mut n = 0;
        let keys: Vec<String> = self.cache.fetched.keys().cloned().collect();
        for key in keys {
            if budget.is_some_and(|b| t0.elapsed() >= b) {
                break;
            }
            let Some(f) = self.cache.fetched.remove(&key) else { continue };
            self.take_in(key, f);
            n += 1;
        }
        n
    }

    /// Put one model prepared on a worker thread into the stores and the cache (unless
    /// it was prepared here meanwhile).
    fn take_in(&mut self, key: String, (m, out): Fetched) -> Option<Arc<PreparedModel>> {
        if let Some(m) = self.cache.map.get(&key) {
            return m.clone();
        }
        self.insert_deferred(out);
        let m = match m {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                crate::guard::failed(&mut self.cache.failures, "model", format!("{key}: {e}"));
                None
            }
        };
        self.cache.map.insert(key, m.clone());
        m
    }

    /// [design, #2685] An ANIO model prepared for the named idle's external tracks.
    /// Its separate cache entry preserves ordinary placed-model preparation.
    pub fn model_for_idle(&mut self, path: &str, idle: u32, clip: &Clip) -> Option<Arc<PreparedModel>> {
        let path = fnv_vfs::normalize(path);
        let key = format!("{path}|actor-idle-{idle:08x}");
        if let Some(model) = self.cache.map.get(&key) {
            return model.clone();
        }
        let mut stores = Stores {
            textures: StoreTextures { textures: &mut self.textures, data: &self.data, images: &mut self.images, formats: self.formats.as_deref() },
            meshes: &mut self.meshes, materials: &mut self.materials,
            bindposes: &mut self.bindposes, morphs: &mut self.cache.morphs,
        };
        let model = crate::guard::guarded(|| prepare_unguarded(&mut stores, &path, false, false, &[], false, Some(clip)))
            .map(Arc::new).map_err(|e| warn!("anim object: {path}: {e}")).ok();
        self.cache.map.insert(key, model.clone());
        model
    }

    fn prepare(&mut self, path: &str, keep_cpu: bool, first_person: bool, swaps: &[AltTexture]) -> Result<PreparedModel, String> {
        let mut stores = Stores {
            textures: StoreTextures { textures: &mut self.textures, data: &self.data, images: &mut self.images, formats: self.formats.as_deref() },
            meshes: &mut self.meshes,
            materials: &mut self.materials,
            bindposes: &mut self.bindposes,
            morphs: &mut self.cache.morphs,
        };
        prepare_into(&mut stores, path, keep_cpu, first_person, swaps)
    }
}

/// Prepare the model at `path` (normalised) into `sink`'s asset stores: on the main
/// thread ([`Stores`]) or on a worker thread ([`Deferred`]).
/// A panic while preparing is that model's failure, not the game's (guard.rs).
fn prepare_into(sink: &mut impl ModelSink, path: &str, keep_cpu: bool, first_person: bool, swaps: &[AltTexture]) -> Result<PreparedModel, String> {
    crate::guard::guarded(|| prepare_unguarded(sink, path, keep_cpu, first_person, swaps, false, None))
}

fn prepare_unguarded(sink: &mut impl ModelSink, path: &str, keep_cpu: bool, first_person: bool, swaps: &[AltTexture], attachments: bool, external: Option<&Clip>) -> Result<PreparedModel, String> {
    let game = sink.data().clone();
    // Where this file's time goes, for the exit's summary (#3149).
    let mut timed = crate::load_times::Timed::start(&crate::load_times::DRAWN);
    let bytes = game.read(path).ok_or("not in any archive")?;
    timed.read();
    let schema = game.schema.clone();
    let file = NifFile::parse(&bytes, Some(&schema)).map_err(|e| e.to_string())?;
    timed.parsed();
    if file.size_table_ignored {
        // Its size table is wrong; the blocks were split by decoding them (#3198).
        bevy::log::info!("model: {path}: its block-size table does not fit its blocks; read block by block");
    }
    let dec = Decoded::new(&file, &bytes, &schema);
    timed.decoded();
    let objects = scene_objects(&file, &dec);
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let hidden_self = |b: usize| {
        dec.blocks[b].as_ref().ok().and_then(|s| s.int("flags")).unwrap_or(0) & 1 != 0
            || is_editor_marker(by_block.get(&b).and_then(|o| o.name.as_deref()))
    };

    let mut out = PreparedModel::default();
    let mut shape_objects = HashMap::new();
    let (mut nodes, node_index) = model_nodes(&objects);
    for (block, i) in &node_index {
        if nodes[*i].billboard.is_some() {
            nodes[*i].billboard = Some(dec.blocks[*block].as_ref().ok().and_then(|s| s.int("billboard_mode")).unwrap_or(0) as u16);
        }
        if nodes[*i].addon.is_some() {
            nodes[*i].addon = dec.blocks[*block].as_ref().ok().and_then(|s| s.int("value")).map(|v| v as i32);
        }
        if nodes[*i].damage_stages.is_some() {
            let field = |k: &str| dec.blocks[*block].as_ref().ok().and_then(|s| s.int(k)).map(|v| v as i32);
            // Unread: shown once damaged at all, never on the intact model [design].
            nodes[*i].damage_stages = Some(field("min").zip(field("max")).unwrap_or((1, i32::MAX)));
        }
    }
    out.nodes = nodes;
    out.particles = file.blocks.iter().any(|b| matches!(b.type_name.as_str(), "NiParticleSystem" | "BSStripParticleSystem"));
    let root = objects.iter().find(|o| o.parent.is_none()).map(|r| r.block);
    if let Some(r) = root.and_then(|r| by_block.get(&r)) {
        out.root = affine(&r.transform);
    }

    // The embedded sequence played from load: Idle, else SpecialIdle, that moves at
    // least one node of this file.
    let seqs = sequences(&file, &dec);
    let has = |n: &str| objects.iter().any(|o| o.name.as_deref() == Some(n));
    // The file's first named point or spot light (lights are not scene objects).
    let first_light = model_light(&file, &dec);
    let has_light = |n: &str| first_light.as_ref().is_some_and(|l| l.name == n);
    // A clip moves a node of this file, drives the material of one of its shapes, shows
    // and hides one of its nodes, or drives its light.
    let moves = |c: &Clip| {
        c.tracks.iter().any(|(n, _)| has(n)) || c.materials.iter().any(|m| has(&m.shape)) || c.visibility.iter().any(|(n, ..)| has(n)) || c.lights.iter().any(|l| has_light(&l.light))
            // [design, #2685] Morph-only groups also need a rig and a Playing clock.
            || seqs.iter().filter(|s| s.name.as_deref() == Some(&c.name)).any(|s| s.tracks.iter().any(|t| t.controller_type.as_deref() == Some("NiGeomMorpherController") && t.node_name.as_deref().is_some_and(has)))
    };
    let named = |want: &str| {
        seqs.iter().filter(|s| s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(want))).map(Clip::from_sequence).find(&moves)
    };
    let mut idle = named("idle").or_else(|| named("specialidle"));
    // What the model plays as an effect (`nvforever::shot_fx`): its idle sequence or a
    // `SpecialIdle…` one (`SpecialIdle_AreaEffect` in the explosion models), with the
    // controllers no sequence drives.
    let special = || seqs.iter().filter(|s| s.name.as_deref().is_some_and(|n| n.to_ascii_lowercase().starts_with("specialidle"))).map(Clip::from_sequence).find(&moves);
    let effect = effect_clip(&file, &dec, idle.clone().or_else(special)).filter(moves);
    // Controllers that run by themselves (scrolling smoke and dust textures) play
    // with the idle clip, each on its own cycle; a model without one gets a clip of
    // its own for them.
    let free: Vec<MaterialTrack> = free_material_tracks(&file, &dec).into_iter().filter(|t| has(&t.shape)).collect();
    let moving: Vec<(String, Arc<TransformTrack>, OwnCycle)> = free_transform_tracks(&file, &dec).into_iter().filter(|t| has(&t.0)).collect();
    let shows: Vec<(String, BoolTrack, OwnCycle)> = free_visibility_tracks(&file, &dec).into_iter().filter(|t| has(&t.0)).collect();
    if !free.is_empty() || !moving.is_empty() {
        info!("{path}: {} material and {} transform track(s) of controllers of their own, played on their own cycles", free.len(), moving.len());
    }
    if !shows.is_empty() {
        info!("{path}: {} visibility track(s) of controllers of their own, played on their own cycles", shows.len());
    }
    if !free.is_empty() || !moving.is_empty() || !shows.is_empty() {
        match idle.as_mut() {
            Some(c) => {
                c.materials.extend(free);
                c.free.extend(moving);
                c.free_visibility.extend(shows);
            }
            None => idle = Some(Clip::from_controllers(free, moving, shows)),
        }
    }
    // A broken file's clips (NaN keys, timing out of range) are dropped, the model
    // standing still instead.
    let mut idle = idle.and_then(|c| playable(path, c));
    let effect = effect.and_then(|c| playable(path, c));
    if let Some(c) = idle.as_ref().filter(|c| !c.visibility.is_empty()) {
        info!("{path}: its {} sequence shows and hides {} node(s)", c.name, c.visibility.len());
    }
    // [design, #2685] Free morphs keep their shared clock; manager morphs use the rig.
    let morph_of = shape_morphs(&file, &dec);
    let managed_morphs: HashMap<_, _> = geom_morphers(&file, &dec).into_iter().filter_map(|g| {
        let target = usize::try_from(g.target).ok()?;
        let name = by_block.get(&target)?.name.as_deref()?;
        let morph = crate::morph::SequenceMorph::bind(&g, &seqs, name)?;
        Some((target, Arc::new(morph)))
    }).collect();
    // A door's Open and Close, when both move nodes of this file.
    let mut door = named("open").zip(named("close")).and_then(|(open, close)| Some(DoorClips { open: playable(path, open)?, close: playable(path, close)? }));
    // Every named sequence that animates this file (scripts' PlayGroup).
    let mut groups: Vec<Clip> = seqs.iter().filter(|s| s.name.is_some()).map(Clip::from_sequence).filter(&moves).filter_map(|c| playable(path, c)).collect();
    // [design, #2685] Prepare external idle targets before choosing material features
    // and rig nodes, including models whose own controllers only blend KF values.
    if let Some(clip) = external {
        idle = Some(clip.clone());
        groups.push(clip.clone());
    }
    // Shapes that wear a driven shape's material or texturing property are driven with it
    // (`material_sharers`, `texturing_sharers`).
    let sharers = material_sharers(&file, &dec);
    let texturing = texturing_sharers(&file, &dec);
    let mut effect = effect;
    if !sharers.is_empty() || !texturing.is_empty() {
        let names: HashMap<String, String> = objects.iter().filter_map(|o| o.name.clone()).map(|n| (n.to_ascii_lowercase(), n)).collect();
        let none = HashMap::new();
        let (mut shared, mut scrolled): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for c in idle.iter_mut().chain(effect.iter_mut()).chain(door.iter_mut().flat_map(|d| [&mut d.open, &mut d.close])).chain(groups.iter_mut()) {
            for (out, added) in [(&mut shared, share_material_tracks(c, &sharers, &none, &names)), (&mut scrolled, share_material_tracks(c, &none, &texturing, &names))] {
                for s in added {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
        }
        if !shared.is_empty() {
            info!("{path}: shapes sharing a driven material are driven with it: {}", shared.join(", "));
        }
        if !scrolled.is_empty() {
            info!("{path}: shapes sharing a driven texturing property move their texture with it: {}", scrolled.join(", "));
        }
    }
    // Shapes whose material a clip of the rig drives.
    let animated_shapes: HashSet<String> = idle
        .iter()
        .chain(door.iter().flat_map(|d| [&d.open, &d.close]))
        .chain(groups.iter())
        .chain(effect.iter())
        .flat_map(|c| c.materials.iter().map(|m| m.shape.clone()))
        .collect();
    let emissive_shapes: HashSet<String> = idle
        .iter()
        .chain(door.iter().flat_map(|d| [&d.open, &d.close]))
        .chain(groups.iter())
        .chain(effect.iter())
        .flat_map(|c| c.materials.iter().filter(|m| matches!(m.target, MaterialTarget::SelfIllum | MaterialTarget::EmissiveMult)).map(|m| m.shape.clone()))
        .collect();
    let alpha_shapes: HashSet<String> = idle
        .iter()
        .chain(door.iter().flat_map(|d| [&d.open, &d.close]))
        .chain(groups.iter())
        .chain(effect.iter())
        .flat_map(|c| c.materials.iter().filter(|m| m.target == MaterialTarget::Alpha).map(|m| m.shape.clone()))
        .collect();
    // Nodes a clip of the rig shows and hides (`NiVisController`): their shapes are
    // built even when the file hides them, and spawn hidden in the rig.
    let vis_nodes: HashSet<String> = idle
        .iter()
        .chain(door.iter().flat_map(|d| [&d.open, &d.close]))
        .chain(groups.iter())
        .chain(effect.iter())
        .flat_map(|c| c.visibility.iter().map(|(n, ..)| n.clone()).chain(c.free_visibility.iter().map(|(n, ..)| n.clone())))
        .collect();
    // The light a clip of the rig drives.
    out.light = first_light.filter(|l| idle.iter().chain(groups.iter()).any(|c| c.lights.iter().any(|t| t.light == l.name)));
    // Rig nodes: the root first, then every object reachable from it.
    let jointed = file.blocks.iter().any(|b| b.type_name.ends_with("Constraint"));
    out.jointed = jointed;
    out.loose = !jointed && dynamic_bodies(&file, &dec) >= 2;
    let animated = idle.is_some() || door.is_some() || !groups.is_empty() || effect.is_some() || jointed || out.loose;
    let mut rig = animated
        .then(|| {
        let root = root?;
        let mut index: HashMap<usize, usize> = HashMap::from([(root, 0)]);
        let mut order = vec![root];
        let mut i = 0;
        while i < order.len() {
            for &c in &by_block[&order[i]].children {
                if by_block.contains_key(&c) && !index.contains_key(&c) {
                    index.insert(c, order.len());
                    order.push(c);
                }
            }
            i += 1;
        }
        let nodes = order
            .iter()
            .enumerate()
            .map(|(k, b)| {
                let o = by_block[b];
                RigNode {
                    name: o.name.clone(),
                    parent: o.parent.and_then(|p| index.get(&p).copied()),
                    local: if k == 0 { Transform::IDENTITY } else { to_bevy(&o.transform) },
                    hidden: k > 0 && hidden_self(*b),
                    billboard: node_index.get(b).and_then(|&i| out.nodes[i].billboard),
                }
            })
            .collect();
        Some((Rig { nodes, shapes: Vec::new(), idle, door, groups, effect }, index))
        })
        .flatten();

    // Alternate textures: the texture set each shape (by block) takes, entries later in
    // the list winning (texture-swaps.md §2).
    let mut swap_of: HashMap<usize, &[Option<String>; 6]> = HashMap::new();
    if !swaps.is_empty() {
        let order = fnv_nif::scene::geometry_order(&objects);
        let shapes: Vec<(usize, Option<&str>, usize)> =
            objects.iter().filter(|o| !o.is_node).filter_map(|o| Some((o.block, o.name.as_deref(), *order.get(&o.block)?))).collect();
        let named: Vec<(Option<&str>, usize)> = shapes.iter().map(|s| (s.1, s.2)).collect();
        for e in swaps {
            let to = fnv_world::alt_textures::targets(e, &named);
            if to.is_empty() {
                info!("{path}: texture swap: no shape {:?} (index {}) for texture set {:08X}", e.shape, e.index, e.texture_set);
            }
            for i in to {
                swap_of.insert(shapes[i].0, &e.textures);
            }
        }
    }
    let mut swapped: Vec<String> = Vec::new();
    // First person: textures the name rule turned to their 1stperson form, and all the
    // shapes wear in that form after it and the texture swap (first-person-textures.md).
    let mut first_textures = 0;
    let mut first_worn = 0;
    // Shapes without any property beside shaded ones: editor geometry (`Props::is_helper`).
    let shaded = shaded_model(&file, &dec);
    let mut editor_shapes: Vec<String> = Vec::new();
    let mut rejected = NumericRejections::default();
    // Shapes drawn with a window environment map, and how (nif.md §6 "Window glass and
    // environment maps").
    let mut env_shapes: Vec<String> = Vec::new();
    let mut env_how = String::new();
    // Shapes without the window flag drawn with an environment map, each with how
    // (nif.md §6 "Environment maps on other shapes").
    let mut other_env: Vec<String> = Vec::new();
    for o in objects.iter().filter(|o| !o.is_node) {
        // Walk to the root: accumulate the transform below the root's own and look
        // for hidden ancestors.
        let mut t = if o.parent.is_some() { affine(&o.transform) } else { Affine3A::IDENTITY };
        // Hidden in the file, and hidden for good: no visibility track of the rig
        // shows the hidden node.
        let shown_by_track = |b: usize| rig.is_some() && by_block.get(&b).and_then(|x| x.name.as_deref()).is_some_and(|n| vis_nodes.contains(n));
        let mut hidden = hidden_self(o.block);
        let mut for_good = hidden && !shown_by_track(o.block);
        let mut ancestors = Vec::new();
        let mut cur = o.parent;
        let mut depth = 0;
        while let Some(p) = cur {
            let Some(po) = by_block.get(&p) else { break };
            if po.parent.is_some() {
                t = affine(&po.transform) * t;
                ancestors.extend(po.name.clone());
            }
            hidden |= hidden_self(p);
            for_good |= hidden_self(p) && !shown_by_track(p);
            cur = po.parent;
            depth += 1;
            if depth > 512 {
                break;
            }
        }
        if for_good {
            continue;
        }
        let Ok(shape) = &dec.blocks[o.block] else { continue };
        let Some(data) = shape.block_ref("data").and_then(|r| dec.get(r)) else { continue };
        let mut props = shape_props(&file, &dec, o, &by_block);
        let shape_name = o.name.clone().unwrap_or_default();
        // Object LOD (the landscape's LOD chunks and blocks, nvforever lod.rs) takes no
        // authored set (#2981): its atlases are not the textures a pack's sets name.
        props.no_authored = is_lod_model(path);
        props.animated_emissive = emissive_shapes.contains(&shape_name);
        props.animated_alpha = alpha_shapes.contains(&shape_name);
        if props.is_helper(shaded) {
            if props.is_bare() {
                editor_shapes.push(shape_name);
            }
            continue;
        }
        if let Some(set) = swap_of.get(&o.block) {
            if props.use_texture_set(set) > 0 {
                swapped.push(format!("{shape_name} → {}", props.textures.first().map_or("-", String::as_str)));
            }
        }
        if first_person {
            first_textures += props.use_first_person_textures(|p| game.vfs.exists(p));
            first_worn += props.first_person_texture_count();
        }
        let Some(AdmittedShape { mut mesh, triangles: tris, skin, stored }) = rejected.admit(o.block, &dec, shape, data) else { continue };
        let managed_morph = managed_morphs.get(&o.block).filter(|m| m.num_vertices() == mesh.count_vertices()).cloned();
        if managed_morph.is_some() {
            // [design, #2685] Keep the source available for private rig mesh copies.
            mesh.asset_usage = RenderAssetUsages::all();
        }
        // A morphing shape: its mesh stays readable in the main world so that
        // `morph::play_vertex_morphs` can rewrite its vertices.
        let morph = morph_of.get(&o.block).filter(|g| g.num_vertices == mesh.count_vertices()).map(|g| {
            mesh.asset_usage = RenderAssetUsages::all();
            let label = format!("{path} {shape_name}");
            info!("{label}: vertex morphs, {} morph(s) over a {:.2} s cycle ({})", g.morphs.len() - 1, g.stop_time - g.start_time, if g.is_free() { "own controller" } else { "idle sequence" });
            Arc::new(crate::morph::ShapeMorph {
                label,
                base: g.morphs[0].vectors.clone(),
                offsets: g.morphs[1..].iter().map(|m| (m.vectors.clone(), m.weight.clone())).collect(),
                cycle: OwnCycle { start: g.start_time, stop: g.stop_time, frequency: g.frequency.max(1e-6), phase: g.phase, cycle_type: (g.flags >> 1) & 3 },
            })
        });
        // The normal map's tangents: those the file stores (crate::tangents).
        let (material, missing_diffuse) = shape_material_with(&props, &mut mesh, sink, crate::tangents::Tangents::Stored(&stored));
        if !rejected.accept_mesh(o.block, &mesh) {
            continue;
        }
        if material.extension.env.x > 0.0 && material.extension.env_how.z > 0.5 {
            let over = if matches!(material.base.alpha_mode, AlphaMode::Premultiplied) { "over the blend" } else { "added" };
            // Scaled by light only under a debug switch (env_map.rs `LIGHT_WINDOW`); the
            // default line stays #2462's.
            let light = match material.extension.env_how.y {
                y if y > 1.5 => ", by scene light",
                y if y > 0.5 => ", by received light",
                _ => "",
            };
            env_how = format!("{}, scale {:.2}, {over}{light}", props.textures.get(4).map_or("-", String::as_str), material.extension.env.x);
            env_shapes.push(shape_name.clone());
        } else if material.extension.env.x > 0.0 {
            other_env.push(format!("{shape_name} ({})", crate::env_map::describe(props.textures.get(4).map_or("-", String::as_str), &material)));
        }
        if let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            for v in p {
                let q = (out.root * t).transform_point3(Vec3::from_array(*v));
                out.bounds = Some(match out.bounds {
                    None => (q, q),
                    Some((lo, hi)) => (lo.min(q), hi.max(q)),
                });
            }
        }
        if skin.is_some() {
            out.skinned += 1;
        }
        let material = sink.material(material);
        let cpu = keep_cpu.then(|| Arc::new(mesh.clone()));
        // Ranges into the mesh's triangles: only when `build_mesh` kept every one.
        let drawn = mesh.indices().map_or(0, |i| i.len() / 3);
        let segments = segment_triangles(shape, tris).filter(|_| drawn == tris).unwrap_or_default();
        // The rig's copy of a skinned mesh carries the joint attributes; the static
        // copy draws the bind pose without them.
        let rig_skinned = match (&rig, skin) {
            (Some((_, index)), Some(skin)) => {
                let joints: Option<Vec<usize>> = skin.bones.iter().map(|b| index.get(b).copied()).collect();
                joints.map(|joints| {
                    let mut m = mesh.clone();
                    let binds = skin.insert_into(&mut m);
                    let m = m
                        .with_generated_skinned_mesh_bounds()
                        .unwrap_or_else(|_| Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD));
                    (sink.mesh(m), sink.bindposes(binds.into()), joints)
                })
            }
            _ => None,
        };
        let mesh = sink.mesh(mesh);
        if let Some(m) = &morph {
            sink.morph(mesh.id(), m.clone());
            if let Some((skinned, ..)) = &rig_skinned {
                sink.morph(skinned.id(), m.clone());
            }
        }
        if let Some((r, index)) = rig.as_mut() {
            if let Some(&node) = index.get(&o.block) {
                let (mesh, skin) = match rig_skinned {
                    Some((m, binds, joints)) => (m, Some((binds, joints))),
                    None => (mesh.clone(), None),
                };
                let animated = animated_shapes.contains(&shape_name);
                let (e, m) = props.emissive_parts();
                let look = (shape_name.clone(), e, m, props.alpha_part());
                r.shapes.push(RigShape { node, mesh, material: material.clone(), skin, look, refraction: props.refraction_part(), animated, morph: managed_morph, block: o.block as u32 });
            }
        }
        // A shape only a visibility track shows is the rig's alone.
        if hidden {
            continue;
        }
        if attachments { shape_objects.insert(o.block, out.shapes.len()); }
        out.shapes.push(PreparedShape {
            cpu,
            segments,
            mesh,
            material,
            transform: t,
            name: o.name.clone(),
            ancestors,
            node: o.parent.and_then(|p| node_index.get(&p).copied()),
            own: if o.parent.is_some() { affine(&o.transform) } else { Affine3A::IDENTITY },
            external_emittance: props.external_emittance(),
            look: {
                let (e, m) = props.emissive_parts();
                crate::material_anim::Look::stored(e, m, props.alpha_part())
            },
            missing_diffuse,
            block: o.block as u32,
        });
        out.triangles += tris;
    }
    if first_person {
        info!("{path}: first person, {first_worn} texture(s) in their 1stperson form ({first_textures} by the name rule, the rest named by its texture swap or the model)");
    }
    if !editor_shapes.is_empty() {
        info!("{path}: {} editor shape(s) without any property left out: {}", editor_shapes.len(), editor_shapes.join(", "));
    }
    if !env_shapes.is_empty() {
        info!("{path}: {} shape(s) with a window environment map ({env_how}): {}", env_shapes.len(), env_shapes.join(", "));
    }
    if !other_env.is_empty() {
        info!("{path}: {} shape(s) with an environment map, no window flag: {}", other_env.len(), other_env.join("; "));
    }
    if !swaps.is_empty() {
        info!("{path}: texture swap: {} entr(ies), {} shape(s) retextured: {}", swaps.len(), swapped.len(), swapped.join(", "));
    }
    if attachments { out.objects = attachment::objects(&objects, &dec, &shape_objects, rig.as_ref().map(|(_, index)| index)); }
    rejected.report(path);
    out.rig = rig.map(|(r, _)| r);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_nif::scene::Transform as NifTransform;

    #[test]
    fn shapes_sharing_a_driven_material_get_its_tracks() {
        // vintageradio01.nif: Open names the body; the needle and the glass dial wear
        // the same NiMaterialProperty. A texture scroll is not shared.
        let glow = MaterialTrack { shape: "VintageRadioBody:0".into(), target: MaterialTarget::SelfIllum, values: Arc::new(ValueTrack::Point3(Channel::Static(Some([1.0; 3])))), cycle: None, priority: 0 };
        let scroll = MaterialTrack { target: MaterialTarget::UvOffset(0), values: Arc::new(ValueTrack::Float(Channel::Static(Some([0.5])))), ..glow.clone() };
        let mut clip = Clip::from_controllers(vec![glow, scroll], Vec::new(), Vec::new());
        let names: HashMap<String, String> = ["VintageRadioBody:0", "VintageRadioNeedle01", "VintageRadioGlassDial:0"].iter().map(|n| (n.to_ascii_lowercase(), n.to_string())).collect();
        let sharers = HashMap::from([("vintageradiobody:0".to_string(), vec!["vintageradioneedle01".to_string(), "vintageradioglassdial:0".to_string()])]);
        let added = share_material_tracks(&mut clip, &sharers, &HashMap::new(), &names);
        assert_eq!(added, ["VintageRadioNeedle01", "VintageRadioGlassDial:0"]);
        assert_eq!(clip.materials.len(), 4);
        assert!(clip.materials[2..].iter().all(|t| t.target == MaterialTarget::SelfIllum));
        // Again: nothing more (each already driven).
        assert!(share_material_tracks(&mut clip, &sharers, &HashMap::new(), &names).is_empty());
    }

    #[test]
    fn shapes_sharing_a_driven_texturing_property_scroll_with_it() {
        // nvsandstormnight.nif (#2450): SpecialIdle scrolls NVSandstorm01:0's base texture
        // and holds its alpha; the other dust sheets list the same NiTexturingProperty and
        // NiMaterialProperty, so all of them scroll and none is left standing still.
        let scroll = MaterialTrack { shape: "NVSandstorm01:0".into(), target: MaterialTarget::UvOffset(0), values: Arc::new(ValueTrack::Float(Channel::Static(Some([0.5])))), cycle: None, priority: 0 };
        let alpha = MaterialTrack { target: MaterialTarget::Alpha, values: Arc::new(ValueTrack::Float(Channel::Static(Some([1.0])))), ..scroll.clone() };
        let mut clip = Clip::from_controllers(vec![scroll, alpha], Vec::new(), Vec::new());
        let names: HashMap<String, String> = ["NVSandstorm01:0", "NVSandstorm02:0", "NVSandstorm03:0"].iter().map(|n| (n.to_ascii_lowercase(), n.to_string())).collect();
        let sharers = HashMap::from([("nvsandstorm01:0".to_string(), vec!["nvsandstorm02:0".to_string(), "nvsandstorm03:0".to_string()])]);
        let added = share_material_tracks(&mut clip, &sharers, &sharers, &names);
        assert_eq!(added, ["NVSandstorm02:0", "NVSandstorm03:0"]);
        for shape in ["NVSandstorm02:0", "NVSandstorm03:0"] {
            let of = |t: MaterialTarget| clip.materials.iter().filter(|m| m.shape == shape && m.target == t).count();
            assert_eq!((of(MaterialTarget::UvOffset(0)), of(MaterialTarget::Alpha)), (1, 1), "{shape}");
        }
        // Without a shared texturing property the scroll stays on its own shape.
        let mut clip = Clip::from_controllers(vec![clip.materials[0].clone()], Vec::new(), Vec::new());
        assert!(share_material_tracks(&mut clip, &sharers, &HashMap::new(), &names).is_empty());
    }

    #[test]
    fn dynamic_motion_systems_are_those_fnv_collide_simulates() {
        // carhulk01.nif's bodies are 4 (box inertia); the globe's stand is 2 (sphere
        // inertia); 6 is keyframed and 7 fixed, as `fnv_collide::nif::motion` reads them.
        for m in [1, 2, 3, 4, 5, 8, 9] {
            assert!(is_dynamic_motion(Some(m)), "{m}");
        }
        for m in [Some(0), Some(6), Some(7), Some(10), Some(-1), None] {
            assert!(!is_dynamic_motion(m), "{m:?}");
        }
    }

    #[test]
    fn attach_light_takes_the_nodes_above_it() {
        let node = |name: &str, parent, z| ModelNode {
            name: Some(name.into()),
            parent,
            local: Affine3A::from_translation(Vec3::new(0.0, 0.0, z)),
            billboard: None,
            addon: None,
            damage_stages: None,
        };
        // barrel02firelight.nif: AttachLight 102 units up, beside the add-on node.
        let barrel = [node("AddOnNode14", None, 72.5), node("AttachLight", None, 102.0)];
        assert_eq!(attach_light(&barrel).map(|t| t.translation.z), Some(102.0));
        let nested = [node("Dummy", None, 10.0), node("attachlight01", Some(0), 5.0)];
        assert_eq!(attach_light(&nested).map(|t| t.translation.z), Some(15.0));
        assert_eq!(attach_light(&[node("AttachLightBeam", None, 1.0)]), None);
        assert_eq!(attach_light(&[]), None);
    }

    #[test]
    fn nodes_below_damage_stages_take_their_ranges() {
        let node = |parent, damage_stages| ModelNode { name: None, parent, local: Affine3A::IDENTITY, billboard: None, addon: None, damage_stages };
        // motorcycle01.nif's shape: Motorbike > DamageStage1-3 > AddOnNode2, and a
        // range nested in another.
        let nodes = [node(None, None), node(Some(0), Some((1, 3))), node(Some(1), None), node(Some(1), Some((2, 5))), node(Some(0), None)];
        assert_eq!(damage_stages_of(&nodes, 0), None);
        assert_eq!(damage_stages_of(&nodes, 2), Some((1, 3)));
        assert_eq!(damage_stages_of(&nodes, 3), Some((2, 3)));
        assert_eq!(damage_stages_of(&nodes, 4), None);
    }

    #[test]
    fn a_cached_model_goes_once_nothing_draws_it() {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<NifMaterial>::default();
        let shape = PreparedShape {
            mesh: meshes.add(Mesh::from(Cuboid::default())),
            material: materials.add(crate::nif_material::nif_material(StandardMaterial::default())),
            transform: Affine3A::IDENTITY,
            name: None,
            ancestors: Vec::new(),
            node: None,
            own: Affine3A::IDENTITY,
            cpu: None,
            segments: Vec::new(),
            external_emittance: false,
            look: crate::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
            missing_diffuse: None,
            block: 0,
        };
        let mut cache = ModelCache::default();
        cache.map.insert("a.nif".into(), Some(Arc::new(PreparedModel { shapes: vec![shape.clone()], ..default() })));
        cache.map.insert("missing.nif".into(), None);
        let spawned = (shape.mesh.clone(), shape.material.clone());
        drop(shape);
        let (mut sweeps, mut hints) = (crate::release::ModelSweeps::default(), crate::release::ImageHints::default());
        // Drawn: kept however long.
        assert_eq!(cache.release(&mut sweeps, &mut hints, &materials, 0.0, Default::default()), 0);
        assert_eq!(cache.release(&mut sweeps, &mut hints, &materials, 500.0, Default::default()), 0);
        // A rig holding the Arc keeps it too.
        drop(spawned);
        let rig = cache.map["a.nif"].clone();
        assert_eq!(cache.release(&mut sweeps, &mut hints, &materials, 600.0, Default::default()), 0);
        drop(rig);
        // Unused: kept for the grace time, then dropped; the failure stays remembered.
        assert_eq!(cache.release(&mut sweeps, &mut hints, &materials, 700.0, Default::default()), 0);
        assert_eq!(cache.held(), 1);
        assert_eq!(cache.release(&mut sweeps, &mut hints, &materials, 700.0 + crate::release::KEEP_UNUSED_SECS, Default::default()), 1);
        assert_eq!((cache.held(), cache.len()), (0, 1));
    }

    fn object(block: usize, name: &str, parent: Option<usize>, is_node: bool, children: &[usize], x: f32) -> SceneObject {
        SceneObject {
            block,
            type_name: if is_node { "NiNode" } else { "NiTriStrips" }.to_string(),
            name: Some(name.to_string()),
            parent,
            transform: NifTransform { translation: [x, 0.0, 0.0], ..NifTransform::IDENTITY },
            is_node,
            children: children.to_vec(),
        }
    }

    /// A pistol shaped like `10mmpistol.nif`: parts as nodes under the root, each with
    /// its shape, and a part under a part.
    #[test]
    fn nodes_below_the_root_parents_first() {
        let objects = [
            object(0, "Pistol", None, true, &[5, 1, 3], 9.0),
            object(1, "##Slide", Some(0), true, &[2, 6], 5.0),
            object(2, "##Slide:0", Some(1), false, &[], -5.0),
            object(3, "Pistol:0", Some(0), false, &[], 0.0),
            object(5, "##Clip", Some(0), true, &[], 1.0),
            object(6, "##Catch", Some(1), true, &[], 2.0),
        ];
        let (nodes, index) = model_nodes(&objects);
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_deref().unwrap()).collect();
        // The root and the shapes are not nodes of the list; children in the root's order.
        assert_eq!(names, ["##Clip", "##Slide", "##Catch"]);
        assert_eq!(nodes.iter().map(|n| n.parent).collect::<Vec<_>>(), [None, None, Some(1)]);
        assert_eq!((index[&5], index[&1], index[&6]), (0, 1, 2));
        assert_eq!(nodes[1].local.translation.x, 5.0);
        assert!(!index.contains_key(&0) && !index.contains_key(&2));
        // The way down to a node, whatever the case of its name.
        assert_eq!(node_path(&nodes, &["##catch"]), Some(vec![1, 2]));
        assert_eq!(node_path(&nodes, &["Clip", "##Clip"]), Some(vec![0]));
        assert_eq!(node_path(&nodes, &["ProjectileNode"]), None);
    }
}


#[cfg(test)]
mod bc5_model_tests {
    use super::*;
    use crate::nif_spawn::shape_material_with;
    use crate::tangents::Tangents;
    use crate::texture::bc5_tests::{OwnedData, triangle};
    use bevy::render::render_resource::TextureFormat;

    #[test]
    fn deferred_and_store_model_sinks_select_the_same_normal_role() {
        let owned = OwnedData::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<NifMaterial>::default();
        let mut bindposes = Assets::<SkinnedMeshInverseBindposes>::default();
        let mut morphs = crate::morph::MorphMeshes::default();
        let mut deferred = Deferred {
            data: owned.data.clone(),
            providers: Providers {
                meshes: meshes.get_handle_provider(), materials: materials.get_handle_provider(),
                bindposes: bindposes.get_handle_provider(), images: images.get_handle_provider(),
            },
            cached: Arc::new(TextureSnapshot::new()), workers: cache.workers.clone(), bc_supported: true,
            unmasked: HashMap::new(), out: DeferredAssets::default(),
        };
        let mut props = crate::nif_spawn::Props::default();
        props.textures = vec!["old.dds".into(), "signed.dds".into()];
        let worker_material = shape_material_with(&props, &mut triangle(), &mut deferred, Tangents::Generate).0;
        let worker_normal = worker_material.base.normal_map_texture.as_ref().unwrap().clone();
        assert!(deferred.lacks_alpha(&worker_normal));
        let worker_linear = deferred.texture("signed.dds", false).unwrap();
        assert_ne!(worker_normal, worker_linear);
        assert_eq!(cache.take_workers(&mut images, 1.0), 3);
        assert_eq!(images.get(&worker_normal).unwrap().texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(images.get(&worker_linear).unwrap().texture_descriptor.format, TextureFormat::Bc5RgSnorm);
        let main_material = {
            let mut stores = Stores {
                textures: StoreTextures { textures: &mut cache, data: &owned.data, images: &mut images, formats: None },
                meshes: &mut meshes, materials: &mut materials, bindposes: &mut bindposes, morphs: &mut morphs,
            };
            shape_material_with(&props, &mut triangle(), &mut stores, Tangents::Generate).0
        };
        assert_eq!(main_material.base.normal_map_texture.as_ref(), Some(&worker_normal));
        assert_eq!(main_material.base.base_color_texture, worker_material.base.base_color_texture);
        let snapshot = cache.snapshot(&images);
        assert!(snapshot[&("textures\\signed.dds".into(), TextureUsage::Normal)].2);
        deferred.cached = Arc::new(snapshot);
        deferred.workers = Arc::new(WorkerTextures::default());
        assert_eq!(deferred.normal("signed.dds"), Some((worker_normal, true)));
        assert_eq!(cache.take_workers(&mut images, 2.0), 0);
    }

    /// The worker source finds an authored set (#2981) as the main thread does: the members
    /// by their exact paths in their roles, decoded once, the main thread adopting the
    /// worker's handles, a later snapshot answering without a decode.
    #[test]
    fn deferred_and_store_sources_find_the_same_authored_set() {
        let pack = crate::authored::tests::Pack::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::<Image>::default();
        let (meshes, materials, bindposes) = (Assets::<Mesh>::default(), Assets::<NifMaterial>::default(), Assets::<SkinnedMeshInverseBindposes>::default());
        let mut deferred = Deferred {
            data: pack.data.clone(),
            providers: Providers {
                meshes: meshes.get_handle_provider(), materials: materials.get_handle_provider(),
                bindposes: bindposes.get_handle_provider(), images: images.get_handle_provider(),
            },
            cached: Arc::new(TextureSnapshot::new()), workers: cache.workers.clone(), bc_supported: true,
            unmasked: HashMap::new(), out: DeferredAssets::default(),
        };
        use crate::authored::PbrSet;
        let key = deferred.resolve("walls/brick.dds");
        let found = deferred.pbr_set(&key).expect("authored set on the worker");
        let set = found.authored().cloned().expect("authored");
        let normal = set.normal.clone().expect("_n");
        assert!(deferred.lacks_alpha(&normal), "two channels hold no mask");
        assert_eq!(set.scalars.roughness_scale, Some(0.5));
        // A height-only set: its `_p` by its exact path, in the linear role, on the worker.
        let stone_key = deferred.resolve("walls/stone.dds");
        let stone = deferred.pbr_set(&stone_key).expect("height-only set on the worker");
        assert!(matches!(stone, PbrSet::HeightOnly(_)));
        let depth_map = stone.height().and_then(|h| h.depth_map.clone()).expect("_p");
        assert_eq!(cache.take_workers(&mut images, 1.0), 3, "_orm, _n and _p, once each");
        assert_eq!(images.get(&set.orm).unwrap().texture_descriptor.format, TextureFormat::Bc7RgbaUnorm);
        assert_eq!(images.get(&normal).unwrap().texture_descriptor.format, TextureFormat::Bc5RgUnorm);
        assert_eq!(images.get(&depth_map).unwrap().texture_descriptor.format, TextureFormat::Bc4RUnorm);
        assert!(deferred.pbr_set(&deferred.resolve("walls/signed.dds")).is_none(), "the worker refuses a signed _p as the main thread does");
        let mut main = StoreTextures { textures: &mut cache, data: &pack.data, images: &mut images, formats: None };
        assert_eq!(main.pbr_set(&key).as_ref(), Some(&found), "the main thread adopts the worker's handles");
        assert_eq!(main.pbr_set(&stone_key).as_ref(), Some(&stone), "the worker's _p too");
        deferred.cached = Arc::new(cache.snapshot(&images));
        deferred.workers = Arc::new(WorkerTextures::default());
        assert_eq!(deferred.pbr_set(&key), Some(found));
        assert_eq!(deferred.pbr_set(&stone_key), Some(stone));
        assert!(deferred.workers.entries_len() == 0, "a snapshot hit decodes nothing");
    }

    #[test]
    fn shared_texture_snapshot_preserves_cpu_decoded_height_validation() {
        use crate::release::{ImageHints, KeepLimits, Unused};

        let pack = crate::authored::tests::Pack::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::<Image>::default();
        let mut deferred = Deferred {
            data: pack.data.clone(),
            providers: Providers {
                meshes: Assets::<Mesh>::default().get_handle_provider(),
                materials: Assets::<NifMaterial>::default().get_handle_provider(),
                bindposes: Assets::<SkinnedMeshInverseBindposes>::default().get_handle_provider(),
                images: images.get_handle_provider(),
            },
            cached: Arc::new(TextureSnapshot::new()),
            workers: cache.workers.clone(),
            bc_supported: false,
            unmasked: HashMap::new(),
            out: DeferredAssets::default(),
        };
        let aligned = deferred.resolve("walls/stone.dds");
        let off_grid = deferred.resolve("walls/oddbc4.dds");
        let expected = deferred.pbr_set(&aligned).expect("aligned unsigned BC4 is valid on the CPU path");
        assert!(deferred.pbr_set(&off_grid).is_none(), "off-grid unsigned BC4 is refused");
        assert_eq!(cache.take_workers(&mut images, 1.0), 2);

        let snapshot = cache.snapshot(&images);
        for path in [r"textures\pbr\walls\stone_p.dds", r"textures\pbr\walls\oddbc4_p.dds"] {
            let (_, format, _) = &snapshot[&(path.to_string(), TextureUsage::Linear)];
            assert_eq!(*format, TextureFormat::R8Unorm, "both files take the CPU fallback");
        }
        deferred.cached = Arc::new(snapshot);

        // Retire the published worker entries, leaving only the snapshot and the shared
        // source metadata. The decoded format alone cannot distinguish the two files.
        cache.release(&mut Unused::default(), &ImageHints::default(), 32.0, KeepLimits::default());
        assert_eq!(deferred.workers.entries_len(), 0);
        assert_eq!(deferred.pbr_set(&aligned), Some(expected));
        assert!(deferred.pbr_set(&off_grid).is_none(), "the cached size still rejects off-grid BC4");
        assert_eq!(deferred.workers.entries_len(), 0, "snapshot hits decode nothing");
    }
}
