//! Actors assembled from parts into one entity (workstream 5, docs/spec-notes/actors.md
//! §1.2 and §3): one skeleton hierarchy per actor; skinned part shapes bind to the
//! skeleton's bones by name; rigid parts hang from their attachment bone.
//!
//! - Skeletons and parts are prepared once per archive path ([`ActorParts`]): meshes
//!   (with joint attributes for skinned shapes), materials, inverse bind poses, the
//!   attachment bone, and a CPU copy of each mesh (for per-actor variants such as
//!   FaceGen morphs).
//! - Most rigid parts on the head (`Bip01 Head`) have vertices in the skeleton's axes with
//!   the origin at the bone [observed, actors.md §1.2]: a part whose root node stores the
//!   inverse of the bone's bind rotation (eyes, mouth, teeth) is an ordinary child of the
//!   bone; any other head part (hair, beards, hats, whose node rotations are the identity
//!   or cancel) gets that inverse rotation from the attachment. A rigid part on any other
//!   bone (the Pip-Boy models on `Bip01 L ForeTwist`) is in the bone's own axes: an
//!   ordinary child. Node transforms are kept in every case. Bone-local head meshes
//!   keep the full bone frame instead ([hypothesis: actors.md §1.2, #2529];
//!   [design: `rigid_part_frame`] distinguishes the two layouts using combined rigid
//!   geometry so material submeshes share a frame).
//! - Dismemberment caps are hidden: `BSDismemberSkinInstance` partitions that clear part
//!   flag bit 0 [documented: nif.xml BSPartFlag bit 0 "PF_EDITOR_VISIBLE"; observed: the
//!   cap shapes of `upperbody.nif` (`bodycaps`, `limbcaps`, `meatneck01`, `meathead01`)
//!   all have it clear, its body shapes set it; hypothesis: an intact actor shows no cap].
//!   A shape all of caps is a cap shape ([`PartShape::cap`]); a cap partition in a shape
//!   with drawn ones (the radscorpion's `meatcapbody`) is hidden by itself
//!   ([`Partition::cap`], dismemberment.md §2.5).
//! - Pose: the skeleton's bind pose, frozen (owner decision 2026-10-08), by default; a
//!   KF clips can play on the skeleton's bones instead ([`blend_player`], actors.md §7–8).
//! - Creatures (actors.md §10.2) use the same assembly with one difference for unskinned
//!   shapes ([`PartUse::by_ancestor`]): such a shape hangs from its nearest ancestor in
//!   the part file whose name is a node of the skeleton, keeping the transforms below
//!   that ancestor; else from the part's `Prn`, as an ordinary child on every bone (the
//!   head rule above is for NPC head parts: under it a dog's eyes sit on one side of its
//!   head); else from the skeleton's root.

use crate::anim_play::{BlendPlayer, BoundClip, Clip};
use crate::data::GameData;
use crate::model::ModelCtx;
use crate::nif_material::NifMaterial;
use crate::model::ModelSink;
use crate::nif_spawn::{affine, is_editor_marker, shaded_model, shape_material_reusing_tangents, shape_material_with, shape_props, to_bevy, AdmittedShape, NumericRejections, Props};
use crate::tangents::Tangents;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::math::Affine3A;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use fnv_nif::scene::{scene_objects, Decoded, SceneObject};
use fnv_nif::{NifFile, Value};
use bevy::tasks::{futures::check_ready, Task};
use std::collections::HashMap;
use std::sync::Arc;

/// The bone a rigid part without `Prn` hangs from [hypothesis, actors.md §1.2].
pub const DEFAULT_RIGID_BONE: &str = "Bip01 Head";

/// Minimum alignment of a rigid part's bounds centre with bone-local up before
/// choosing that frame [design, #2529; docs/spec-notes/actors.md §1.2].
pub const BONE_LOCAL_HEAD_ALIGNMENT: f32 = 0.9;

/// A skeleton's node hierarchy; node 0 is the root, whose own transform the actor's
/// placement replaces (so it is the identity here).
#[derive(Debug, Clone, Default)]
pub struct PreparedSkeleton {
    pub names: Vec<Option<String>>,
    pub parents: Vec<Option<usize>>,
    /// Local transforms in NIF (game) space.
    pub locals: Vec<Transform>,
    /// Bind-pose world transforms relative to the root, NIF space.
    pub bind_world: Vec<Affine3A>,
    pub by_name: HashMap<String, usize>,
    /// Named BSBound BBX half-width in skeleton units [documented:
    /// schemas/nif/scene.json; observed: issue #2535 record/file facts].
    pub bbx_half_width: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct PartShape {
    pub name: Option<String>,
    pub mesh: Handle<Mesh>,
    pub material: Handle<NifMaterial>,
    /// The shape's properties, to rebuild its material with another base texture.
    pub(crate) props: Props,
    /// CPU copy (tangents and joint attributes included).
    pub cpu: Arc<Mesh>,
    /// Inverse bind poses and the names of the bones, for skinned shapes.
    pub skin: Option<(Handle<SkinnedMeshInverseBindposes>, Vec<String>)>,
    /// Shape transform below the part's root node (the root's own transform excluded,
    /// see [`PreparedPart::root`]), NIF space; used for rigid shapes.
    pub transform: Affine3A,
    /// A dismemberment cap: drawn with no triangles on an intact actor (its `mesh` has
    /// none; `cpu` has them all).
    pub cap: bool,
    /// Its dismemberment partitions (body part and triangles), for drawing it without a
    /// gone part or with only some caps (docs/spec-notes/dismemberment.md §5).
    pub partitions: Arc<Vec<Partition>>,
    /// The shape's ancestors in the part file, from its parent up to (not including) the
    /// root: node name and local transform (NIF space). `transform` is their product
    /// (outermost first) times the shape's own transform, `own`.
    pub ancestors: Vec<(Option<String>, Affine3A)>,
    pub own: Affine3A,
    /// Its position among the file's geometry blocks in depth-first order
    /// (`fnv_nif::scene::geometry_order`): what a record's alternate textures name it by
    /// besides its name (docs/spec-notes/texture-swaps.md §2).
    pub geometry_index: usize,
}

impl PartShape {
    /// The look its material stores, for clips that drive it (`material_anim`).
    pub fn stored_look(&self) -> crate::material_anim::StoredLook {
        let (emissive, mult) = self.props.emissive_parts();
        crate::material_anim::StoredLook(crate::material_anim::Look::stored(emissive, mult, self.props.alpha_part()))
    }

    /// The diffuse texture the model names (NIF texture path), if any.
    pub fn base_texture(&self) -> Option<&str> {
        self.props.textures.first().map(String::as_str).filter(|t| !t.is_empty())
    }

    /// The shape is skin: its shader property has the skin shader type
    /// (`nif_spawn::SHADER_SKIN`).
    pub fn is_skin(&self) -> bool {
        self.props.skin
    }

    /// The shape uses the hair shader: BSShaderFlags bit 18 "Hair" [documented: nif.xml]
    /// (the hair models, and beards among the head parts: `beardmustachecurly.nif`
    /// [observed, docs/spec-notes/actors.md §3]).
    pub fn is_hair(&self) -> bool {
        self.props.hair
    }
}

/// Per shape, given as (cap, skin), whether a texture given for the whole part (a race
/// part's ICON, the EYES or HAIR texture) replaces its diffuse: the skin shapes when the
/// part has any, else every shape; never a dismemberment cap. The race upper bodies hold
/// the underwear as a second shape with its own texture (`upperbody.nif`: `Arms:0`,
/// `armor\underwear\underwearm.dds`), which the skin texture must not cover; mouth,
/// teeth, tongue and eye models have one shape, not of the skin type, and some name no
/// texture of their own [observed: `crates/fnv-nif/examples/skin_survey.rs`; hypothesis
/// for the rule, docs/spec-notes/actors.md §3.1].
pub fn texture_targets(shapes: &[(bool, bool)]) -> Vec<bool> {
    let any_skin = shapes.iter().any(|&(cap, skin)| !cap && skin);
    shapes.iter().map(|&(cap, skin)| !cap && (skin || !any_skin)).collect()
}

/// The normal map a shape takes when its diffuse is replaced by `texture`, or `None` to
/// keep the one its model names: for a skin shape, the file beside `texture` named with
/// `_n` ([`fnv_vfs::normal_map_beside`]) when `exists` finds it (asked with the VFS
/// path). The race part models and the worn models name the adult human's skin maps
/// (every head model names `characters\male\headhuman_n.dds`) while each kind of skin
/// has its own beside its diffuse, most of them named by no mesh. Shapes that are not
/// skin keep theirs: eye textures have no such file, the mouth, teeth and tongue ones a
/// 1 × 1 stand-in where the models name a flat map, and for hair the files do not show
/// which map the game uses [observed: `crates/fnv-nif/examples/texture_refs.rs`;
/// hypothesis for the rule, docs/spec-notes/actors.md §3.1 "Normal maps"].
pub fn skin_normal_map(skin: bool, texture: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let normal = fnv_vfs::normal_map_beside(texture).filter(|_| skin)?;
    exists(&fnv_vfs::texture_path(&normal)).then_some(normal)
}

#[derive(Debug, Clone, Default)]
pub struct PreparedPart {
    pub shapes: Vec<PartShape>,
    /// `Prn`: the bone rigid shapes hang from.
    pub prn: Option<String>,
    /// The root node's own transform (NIF space).
    pub root: Affine3A,
    /// The part's `ProjectileNode…` nodes (where a creature's body weapon fires from)
    /// with their transforms in the part's space, root transform included: read here so
    /// that drawing a creature does not prepare the file again as a model to find them
    /// (#1429: 23.8 ms in one frame for one creature [observed 2026-11-07, Sierra Madre
    /// lobby]).
    pub projectile_nodes: Vec<(String, Affine3A)>,
    /// Where a held weapon's shots leave: the first node named as in [`MUZZLE_NODES`],
    /// in the part's space, root transform included (as nvforever `weapons::spawn_muzzle`
    /// placed it from the file prepared again as a model). Read here, on the part's
    /// worker thread, for the same reason as `projectile_nodes` (#3057: 28-30 ms of the
    /// frame an armed NPC was drawn in [observed 2026-11-20, `profile hitch` spans]).
    pub muzzle: Option<Affine3A>,
    /// The `UPB` string of each node that has one, by node name: the bone a weapon's
    /// part hangs from (nvforever `weapons::spawn_model`). Read here for the same reason.
    pub upb: Arc<HashMap<String, String>>,
    /// No shape is skinned and the file has controllers of its own (not driven by a
    /// sequence) that move a node, drive a material or show and hide a node: a creature
    /// draws such a part as a model playing them on its bone, not as still shapes
    /// (nvforever `actors::spawn_creature`).
    pub plays_itself: bool,
}

/// The nodes a held weapon's shots leave from, by either of their names (animation.md §9.9).
pub const MUZZLE_NODES: [&str; 2] = ["ProjectileNode", "##ProjectileNode"];

/// Whether a node is named as a projectile node (`ProjectileNode…`, WEAP NNAM).
pub fn is_projectile_node(name: &str) -> bool {
    name.get(..14).is_some_and(|p| p.eq_ignore_ascii_case("ProjectileNode"))
}

impl PreparedPart {
    /// Per shape, whether a texture given for the whole part replaces its diffuse
    /// ([`texture_targets`]).
    pub fn texture_targets(&self) -> Vec<bool> {
        texture_targets(&self.shapes.iter().map(|s| (s.cap, s.is_skin())).collect::<Vec<_>>())
    }

    pub fn attach_bone(&self) -> &str {
        self.prn.as_deref().unwrap_or(DEFAULT_RIGID_BONE)
    }
}

/// Prepared skeletons and parts by archive path; `None` remembers a failure. Materials
/// with a replaced base texture are cached by (path, shape, texture).
#[derive(Resource, Default)]
pub struct ActorParts {
    skeletons: HashMap<String, Option<Arc<PreparedSkeleton>>>,
    parts: HashMap<String, Option<Arc<PreparedPart>>>,
    materials: HashMap<(String, usize, String), Handle<NifMaterial>>,
    clips: HashMap<String, Option<Arc<Clip>>>,
    /// Skeletons read and parsed on a worker thread ([`ActorCtx::parts_ready`]), waiting
    /// for [`ActorCtx::skeleton`], with when they came in; and the reads under way.
    fetched: HashMap<String, (Nif, f64)>,
    fetching: HashMap<String, Task<Fetched>>,
    /// Parts being prepared whole on a worker thread ([`ActorCtx::parts_ready`], #3072),
    /// taken in by [`ActorParts::take_fetched`].
    preparing: HashMap<String, Task<PartPrepared>>,
    /// KF clips being read on a worker thread ([`ActorCtx::clip_ready`]).
    clip_reads: HashMap<String, Task<Result<Clip, String>>>,
    /// Textures an actor's materials name beyond its parts' own (a part's or the race's
    /// skin texture and the normal map beside it) being read on a worker thread
    /// ([`ActorCtx::textures_ready`]), by resolved path; and those whose read gave
    /// nothing (so an actor does not wait for them).
    texture_reads: HashMap<String, Task<TextureFiles>>,
    textures_unread: std::collections::HashSet<String>,
    pub failures: Vec<String>,
}

/// Why a NIF or KF file was not read when no archive or loose file has it ([`decode`]).
const ABSENT: &str = "not in any archive";

/// The first controller sequence of the KF file at `path`.
/// A panic while reading it, or a clip whose keys could not be played, is this clip's
/// failure (guard.rs); a file the game does not have is [`ABSENT`].
fn read_clip(data: &GameData, path: &str) -> Result<Clip, String> {
    let (file, _, dec) = decode(data, path)?;
    crate::guard::guarded(|| {
        let clip = fnv_nif::anim::sequences(&file, &dec).first().map(Clip::from_sequence).ok_or("no sequence")?;
        clip.check()?;
        Ok(clip)
    })
}

/// Where KF clips come from: the actor part cache on the main thread ([`ActorCtx`]), or a
/// [`ClipReader`] on a worker thread (clip sets loaded ahead of the actors that use them).
pub trait ClipSource {
    /// The game's files.
    fn data(&self) -> &GameData;
    /// The first controller sequence of the KF file at `path` (loaded on first use).
    fn clip(&mut self, path: &str) -> Option<Arc<Clip>>;
}

impl ClipSource for ActorCtx<'_, '_> {
    fn data(&self) -> &GameData {
        &self.models.data
    }

    fn clip(&mut self, path: &str) -> Option<Arc<Clip>> {
        ActorCtx::clip(self, path)
    }
}

/// Clips read off the main thread, kept for the reader's life.
pub struct ClipReader {
    data: GameData,
    clips: HashMap<String, Option<Arc<Clip>>>,
}

impl ClipReader {
    pub fn new(data: GameData) -> ClipReader {
        ClipReader { data, clips: HashMap::new() }
    }
}

impl ClipSource for ClipReader {
    fn data(&self) -> &GameData {
        &self.data
    }

    fn clip(&mut self, path: &str) -> Option<Arc<Clip>> {
        let key = fnv_vfs::normalize(path);
        let data = &self.data;
        self.clips.entry(key).or_insert_with_key(|key| read_clip(data, key).ok().map(Arc::new)).clone()
    }
}

/// A NIF file read and parsed: the file, its bytes and its decoded blocks.
type Nif = Result<(NifFile, Vec<u8>, Decoded), String>;

/// A part read off the main thread, with the bytes of the textures its texture sets name
/// (for [`TextureCache::offer`]).
/// Texture files read on a worker thread: (resolved path, bytes).
type TextureFiles = Vec<(String, Vec<u8>)>;

struct Fetched {
    nif: Nif,
    textures: Vec<(String, Vec<u8>)>,
}

/// A part prepared on a worker thread: the part (or why not), its meshes, materials and
/// skins under handles reserved from the stores, and the textures it could not load.
type PartPrepared = (Result<PreparedPart, String>, crate::model::DeferredAssets, Vec<String>);

/// Seconds a part read ahead waits for its actor before it is dropped (an actor that
/// left view before it was drawn).
const FETCHED_KEEP_SECS: f64 = 30.0;

/// Read and parse the part at `path` and read the textures its texture sets name (on a
/// worker thread).
fn fetch(data: &GameData, path: &str) -> Fetched {
    let nif = decode(data, path);
    let mut textures = Vec::new();
    if let Ok((file, _, dec)) = &nif {
        let mut seen = std::collections::HashSet::new();
        for (i, b) in file.blocks.iter().enumerate() {
            let Some(Ok(set)) = dec.blocks.get(i).filter(|_| b.type_name == "BSShaderTextureSet") else { continue };
            for t in crate::nif_spawn::text_list(set, "textures").iter().filter(|t| !t.is_empty()) {
                let path = crate::texture::resolved_path(t, |p| data.vfs.exists(p));
                if seen.insert(path.clone()) && data.vfs.exists(&path) {
                    textures.extend(data.read(&path).map(|bytes| (path, bytes)));
                }
            }
        }
    }
    Fetched { nif, textures }
}

impl ActorParts {
    /// Take in what worker threads finished: parts prepared (their assets into `models`'
    /// stores, the part into the cache) and skeletons read; drop skeletons nobody took in
    /// time. The number taken in.
    pub fn take_fetched(&mut self, models: &mut ModelCtx, now: f64) -> usize {
        let mut prepared = Vec::new();
        self.preparing.retain(|path, task| match check_ready(task) {
            Some(p) => {
                prepared.push((path.clone(), p));
                false
            }
            None => true,
        });
        let mut n = prepared.len();
        for (path, (part, assets, failures)) in prepared {
            // Prepared on the main thread meanwhile (its actor was drawn first): the
            // worker's copy is dropped, its reserved handles with it.
            if self.parts.contains_key(&path) {
                continue;
            }
            models.put_in(assets, now);
            for f in failures {
                self.failures.push(f);
            }
            let part = match part {
                Ok(p) => Some(Arc::new(p)),
                Err(e) => {
                    crate::guard::failed(&mut self.failures, "actor file", format!("{path}: {e}"));
                    None
                }
            };
            self.parts.insert(path, part);
        }
        let textures = &mut *models.textures;
        let mut done = Vec::new();
        self.fetching.retain(|path, task| match check_ready(task) {
            Some(f) => {
                done.push((path.clone(), f));
                false
            }
            None => true,
        });
        n += done.len();
        for (path, f) in done {
            for (t, bytes) in f.textures {
                textures.offer(t, bytes, now);
            }
            self.fetched.insert(path, (f.nif, now));
        }
        self.fetched.retain(|_, (_, at)| now - *at < FETCHED_KEEP_SECS);
        let mut read = Vec::new();
        self.texture_reads.retain(|path, task| match check_ready(task) {
            Some(files) => {
                read.push((path.clone(), files));
                false
            }
            None => true,
        });
        for (path, files) in read {
            if !files.iter().any(|(p, _)| *p == path) {
                self.textures_unread.insert(path);
            }
            for (t, bytes) in files {
                textures.offer(t, bytes, now);
            }
        }
        n
    }

    /// Where the part (or skeleton) at `path` stands: prepared (or failed to be), read on
    /// a worker thread and waiting to be prepared, being read, or none of these.
    pub fn part_state(&self, path: &str) -> PartState {
        let key = fnv_vfs::normalize(path);
        if self.parts.contains_key(&key) || self.skeletons.contains_key(&key) {
            PartState::Prepared
        } else if self.fetched.contains_key(&key) {
            PartState::Read
        } else if self.fetching.contains_key(&key) || self.preparing.contains_key(&key) {
            PartState::Reading
        } else {
            PartState::Unknown
        }
    }
}

/// Where a part stands in [`ActorParts`] ([`ActorParts::part_state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartState {
    Prepared,
    Read,
    Reading,
    Unknown,
}

/// The asset stores actors are prepared into: the static models' ([`ModelCtx`]), so a
/// system can use both, plus the actor part cache.
pub struct ActorCtx<'a, 'w> {
    pub cache: &'a mut ActorParts,
    pub models: &'a mut ModelCtx<'w>,
}

/// One part to assemble, with its per-actor choices.
#[derive(Debug, Clone)]
pub struct PartUse {
    pub part: Arc<PreparedPart>,
    /// Per shape, the material to draw it with (overrides already applied).
    pub materials: Vec<Handle<NifMaterial>>,
    /// Per shape, a second material the shape is drawn with again, from an entity of its
    /// own beside the first (the blended layer of a hair model over its solid core,
    /// `nif_spawn::hair_model_layer`); empty or `None`: drawn once.
    pub layers: Vec<Option<Handle<NifMaterial>>>,
    /// Per shape, a replacement mesh (e.g. FaceGen-morphed), or `None` for the prepared one.
    pub meshes: Vec<Option<Handle<Mesh>>>,
    /// Shape names not to draw (e.g. the hair's `Hat` shape when no headgear is worn).
    pub hidden_shapes: Vec<String>,
    /// Attach unskinned shapes by their ancestors' names (creatures) instead of the
    /// part's `Prn` (NPCs).
    pub by_ancestor: bool,
}

/// Read and parse a NIF or KF; a panic while parsing is this file's failure (guard.rs).
fn decode(data: &GameData, path: &str) -> Result<(NifFile, Vec<u8>, Decoded), String> {
    crate::guard::guarded(|| {
        let bytes = data.read(path).ok_or(ABSENT)?;
        let schema = data.schema.clone();
        let file = NifFile::parse(&bytes, Some(&schema)).map_err(|e| e.to_string())?;
        let dec = Decoded::new(&file, &bytes, &schema);
        Ok((file, bytes, dec))
    })
}

/// Root first, then every object reachable from it, breadth first.
fn hierarchy(objects: &[SceneObject]) -> Vec<usize> {
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let Some(root) = objects.iter().find(|o| o.parent.is_none()) else { return Vec::new() };
    let mut order = vec![root.block];
    let mut i = 0;
    while i < order.len() {
        for &c in &by_block[&order[i]].children {
            if by_block.get(&c).is_some_and(|o| o.is_node) && !order.contains(&c) {
                order.push(c);
            }
        }
        i += 1;
    }
    order
}

/// A skin partition's body part (nif.xml `BSDismemberBodyPartType`, documentation only):
/// 0–13 body parts (BP_TORSO, BP_HEAD, …), 101–113 section caps, 201–213 torso caps, and
/// 1000 × part for torso sections (docs/spec-notes/dismemberment.md §2.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub body_part: u16,
    /// A cap partition: every partition of a cap shape, and in a shape with drawn
    /// partitions a cap body part (1xx, 2xx) with part flag bit 0 (editor visible)
    /// clear. Not drawn on an intact actor (the radscorpion's `meatcapbody` holds one
    /// editor-visible triangle among its torso caps, Mr. Gutsy's body one torso cap;
    /// dismemberment.md §2.5).
    pub cap: bool,
    /// Its triangles, as indices into the shape's mesh vertices (3 per triangle).
    pub indices: Vec<u32>,
}

/// The dismemberment partitions of a skinned shape: the body part each partition of its
/// `BSDismemberSkinInstance` names, with that partition's triangles from its
/// `NiSkinPartition` (the two lists match by index), mapped to the shape's vertices by
/// the partition's vertex map. Empty for other shapes, or when the lists disagree.
fn shape_partitions(dec: &Decoded, shape: &fnv_nif::Struct, vertices: usize) -> Vec<Partition> {
    let Some(si) = shape.block_ref("skin_instance").and_then(|r| dec.get(r)) else { return Vec::new() };
    let Some(parts) = si.list("partitions") else { return Vec::new() };
    let Some(sp) = si.block_ref("skin_partition").and_then(|r| dec.get(r)).and_then(|p| p.list("partitions")) else { return Vec::new() };
    if parts.len() != sp.len() {
        return Vec::new();
    }
    let ints = |v: Option<&Value>| -> Vec<i64> {
        match v {
            Some(Value::IntList(l)) => l.clone(),
            Some(Value::List(l)) => l.iter().filter_map(Value::as_int).collect(),
            _ => Vec::new(),
        }
    };
    let mut out = Vec::new();
    for (p, s) in parts.iter().zip(sp) {
        let (Value::Struct(p), Value::Struct(s)) = (p, s) else { return Vec::new() };
        let body_part = p.int("body_part").unwrap_or(0) as u16;
        let cap = p.int("part_flag").unwrap_or(1) & 1 == 0;
        let map = ints(s.get("vertex_map"));
        let local = |i: i64| -> Option<u32> {
            let v = if map.is_empty() { i } else { *map.get(usize::try_from(i).ok()?)? };
            usize::try_from(v).ok().filter(|&v| v < vertices).map(|v| v as u32)
        };
        let mut tris: Vec<[i64; 3]> = ints(s.get("triangles")).as_chunks::<3>().0.to_vec();
        // Strips: a triangle per window of three, every other one turned; degenerate
        // ones dropped.
        if let Some(Value::List(strips)) = s.get("strips") {
            for strip in strips {
                let st = ints(Some(strip));
                for (k, w) in st.windows(3).enumerate() {
                    if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
                        continue;
                    }
                    tris.push(if k % 2 == 0 { [w[0], w[1], w[2]] } else { [w[0], w[2], w[1]] });
                }
            }
        }
        let indices = tris.iter().filter_map(|t| Some([local(t[0])?, local(t[1])?, local(t[2])?])).flatten().collect();
        out.push(Partition { body_part, cap, indices });
    }
    out
}

/// Whether a partition is a cap ([`Partition::cap`]): in a cap shape always; else a cap
/// body part (1xx, 2xx) whose part flag bit 0 is clear (`flag_clear`).
fn partition_is_cap(cap_shape: bool, flag_clear: bool, body_part: u16) -> bool {
    cap_shape || (flag_clear && (100..300).contains(&body_part))
}

/// Whether the mixed-cap shape `block` of `path` is reported for the first time (the line
/// is written once per model and shape, not each time the part is prepared again after a
/// release).
fn first_mixed_report(path: &str, block: usize) -> bool {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<(String, usize)>>> = std::sync::OnceLock::new();
    SEEN.get_or_init(Default::default).lock().is_ok_and(|mut s| s.insert((path.to_string(), block)))
}

/// Whether a skinned shape is a dismemberment cap: its skin instance has partitions and
/// none of them sets part flag bit 0.
fn is_cap(dec: &Decoded, shape: &fnv_nif::Struct) -> bool {
    let Some(si) = shape.block_ref("skin_instance").and_then(|r| dec.get(r)) else { return false };
    let Some(parts) = si.list("partitions") else { return false };
    !parts.is_empty()
        && parts.iter().all(|p| match p {
            Value::Struct(p) => p.int("part_flag").unwrap_or(1) & 1 == 0,
            _ => false,
        })
}

impl ActorParts {
    /// Drop the prepared parts and retextured materials nothing else holds, after their
    /// grace time (`crate::release`); the numbers dropped (parts, materials). A
    /// retextured material that fell back to the part's own counts as the cache's copy.
    pub fn release(
        &mut self,
        parts: &mut crate::release::Unused<String>,
        materials: &mut crate::release::Unused<(String, usize, String)>,
        hints: &mut crate::release::ImageHints,
        stores: &Assets<NifMaterial>,
        now: f64,
        keep: crate::release::KeepLimits,
    ) -> (usize, usize) {
        use crate::release::Holders;
        let own_material = |k: &(String, usize, String)| self.parts.get(&k.0).and_then(|p| p.as_ref()).and_then(|p| p.shapes.get(k.1)).map(|s| &s.material);
        let entries: Vec<_> = self
            .materials
            .iter()
            .map(|(k, m)| {
                let mut h = Holders::default();
                h.add(m);
                if let Some(o) = own_material(k).filter(|o| *o == m) {
                    h.add(o);
                }
                (k.clone(), h.held_elsewhere())
            })
            .collect();
        let gone = materials.sweep_timed(now, entries, keep.secs, keep.max, |_| None);
        for (k, since) in &gone {
            if let Some(m) = self.materials.remove(k) {
                hints.note(stores, &m, *since, now);
            }
        }
        let entries: Vec<_> = self
            .parts
            .iter()
            .filter_map(|(k, p)| p.as_ref().map(|p| (k, p)))
            .map(|(k, p)| {
                if Arc::strong_count(p) > 1 {
                    return (k.clone(), true);
                }
                let mut h = Holders::default();
                for s in &p.shapes {
                    h.add(&s.mesh);
                    h.add(&s.material);
                    if let Some((b, _)) = &s.skin {
                        h.add(b);
                    }
                }
                for (mk, m) in &self.materials {
                    if mk.0 == *k && p.shapes.get(mk.1).is_some_and(|s| s.material == *m) {
                        h.add(m);
                    }
                }
                (k.clone(), h.held_elsewhere())
            })
            .collect();
        let drop = parts.sweep_timed(now, entries, keep.secs, keep.max, |_| None);
        for (k, since) in &drop {
            if let Some(Some(p)) = self.parts.remove(k) {
                for s in &p.shapes {
                    hints.note(stores, &s.material, *since, now);
                }
            }
        }
        (drop.len(), gone.len())
    }
}

impl ActorCtx<'_, '_> {
    /// The prepared skeleton at `path` (loaded on first use).
    pub fn skeleton(&mut self, path: &str) -> Option<Arc<PreparedSkeleton>> {
        let key = fnv_vfs::normalize(path);
        if let Some(s) = self.cache.skeletons.get(&key) {
            return s.clone();
        }
        let ahead = self.cache.fetched.remove(&key).map(|(nif, _)| nif);
        let s = match crate::guard::guarded(|| prepare_skeleton(ahead.unwrap_or_else(|| decode(&self.models.data, &key)))) {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                crate::guard::failed(&mut self.cache.failures, "actor file", format!("{key}: {e}"));
                None
            }
        };
        self.cache.skeletons.insert(key, s.clone());
        s
    }

    /// Whether every part at `paths` is prepared or read ahead, so preparing them reads
    /// no file on this thread. The rest are read (and their textures) on a worker thread,
    /// taken in by [`ActorParts::take_fetched`]: an actor can wait for them before it is
    /// drawn instead of stalling the frame it comes into view (the parse was a sixth of
    /// the hitch then [observed: CPU profile, 2026-10-22]).
    ///
    /// [design] #3072: a part is prepared whole there (meshes, tangents, textures decoded,
    /// materials and skins, under handles reserved from the stores), as a cell's models
    /// are (`ModelCtx::models_ready`); only putting its assets in stays on this thread.
    /// The skeleton, which makes no assets, is read and parsed there and prepared here.
    pub fn parts_ready<'p>(&mut self, parts: impl IntoIterator<Item = &'p str>, skeleton: &str) -> bool {
        let mut ready = true;
        for path in parts {
            let key = fnv_vfs::normalize(path);
            if self.cache.parts.contains_key(&key) {
                continue;
            }
            ready = false;
            if self.cache.preparing.contains_key(&key) {
                continue;
            }
            let mut sink = self.models.deferred();
            let job = key.clone();
            let task = crate::read_pool::read_pool().spawn(async move {
                let mut failures = Vec::new();
                let nif = decode(sink.data(), &job);
                let part = crate::guard::guarded(|| prepare_part_into(&mut sink, &job, nif, &mut failures));
                (part, sink.into_assets(), failures)
            });
            self.cache.preparing.insert(key, task);
        }
        let key = fnv_vfs::normalize(skeleton);
        if !(self.cache.skeletons.contains_key(&key) || self.cache.fetched.contains_key(&key)) {
            ready = false;
            if !self.cache.fetching.contains_key(&key) {
                let (data, job) = ((*self.models.data).clone(), key.clone());
                let task = crate::read_pool::read_pool().spawn(async move { fetch(&data, &job) });
                self.cache.fetching.insert(key, task);
            }
        }
        ready
    }

    /// Whether the textures at the NIF texture paths `paths` are loaded or read ahead, so
    /// that a material made with one ([`Self::material_with_texture`]) reads no file on this
    /// thread. The rest are read, with the normal map beside each, on a worker thread
    /// (once), taken in by [`ActorParts::take_fetched`]. Missing files count as ready.
    /// Reading a race's skin texture there cost an NPC 5-6 ms in the frame it was drawn
    /// [observed 2026-11-07, #1429, Sierra Madre lobby].
    pub fn textures_ready<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>) -> bool {
        let mut ready = true;
        for raw in paths {
            let vfs = &self.models.data.vfs;
            let path = crate::texture::resolved_path(raw, |p| vfs.exists(p));
            if !vfs.exists(&path) || self.cache.textures_unread.contains(&path) || self.models.textures.has_or_ahead(&path) {
                continue;
            }
            ready = false;
            if self.cache.texture_reads.contains_key(&path) {
                continue;
            }
            let normal = fnv_vfs::normal_map_beside(&path).filter(|n| vfs.exists(n) && !self.models.textures.has_or_ahead(n));
            let (data, job) = ((*self.models.data).clone(), path.clone());
            let task = crate::read_pool::read_pool().spawn(async move { [Some(job), normal].into_iter().flatten().filter_map(|p| data.read(&p).map(|b| (p, b))).collect() });
            self.cache.texture_reads.insert(path, task);
        }
        ready
    }

    /// The prepared part at `path` (loaded on first use).
    pub fn part(&mut self, path: &str) -> Option<Arc<PreparedPart>> {
        let key = fnv_vfs::normalize(path);
        if let Some(p) = self.cache.parts.get(&key) {
            return p.clone();
        }
        let p = match crate::guard::guarded(|| self.prepare_part(&key)) {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                crate::guard::failed(&mut self.cache.failures, "actor file", format!("{key}: {e}"));
                None
            }
        };
        self.cache.parts.insert(key, p.clone());
        p
    }

    /// The part at `path` with a record's alternate textures on the shapes they name
    /// (`fnv_world::alt_textures`, docs/spec-notes/texture-swaps.md): a copy whose shapes'
    /// properties and materials carry the texture sets, cached by the list. No list: the
    /// plain part.
    pub fn part_swapped(&mut self, path: &str, swaps: &[fnv_world::alt_textures::AltTexture]) -> Option<Arc<PreparedPart>> {
        let plain = self.part(path)?;
        if swaps.is_empty() {
            return Some(plain);
        }
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            swaps.hash(&mut h);
            format!("{}|swap{:016x}", fnv_vfs::normalize(path), h.finish())
        };
        if let Some(p) = self.cache.parts.get(&key) {
            return p.clone();
        }
        let named: Vec<(Option<&str>, usize)> = plain.shapes.iter().map(|s| (s.name.as_deref(), s.geometry_index)).collect();
        let mut set_of: Vec<Option<&[Option<String>; 6]>> = vec![None; plain.shapes.len()];
        for e in swaps {
            let to = fnv_world::alt_textures::targets(e, &named);
            if to.is_empty() {
                info!("{path}: texture swap: no shape {:?} (index {}) for texture set {:08X}", e.shape, e.index, e.texture_set);
            }
            for i in to {
                set_of[i] = Some(&e.textures);
            }
        }
        let mut part = (*plain).clone();
        let mut done = Vec::new();
        for (i, set) in set_of.iter().enumerate() {
            let Some(set) = set else { continue };
            part.shapes[i].material = self.material_with_texture_set(path, &plain, i, set);
            part.shapes[i].props.use_texture_set(set);
            done.push(format!("{} → {}", part.shapes[i].name.as_deref().unwrap_or("-"), part.shapes[i].base_texture().unwrap_or("-")));
        }
        info!("{}: texture swap: {} entr(ies), {} shape(s) retextured: {}", fnv_vfs::normalize(path), swaps.len(), done.len(), done.join(", "));
        let part = Some(Arc::new(part));
        self.cache.parts.insert(key, part.clone());
        part
    }

    /// Whether [`clip`](Self::clip) can give the KF file at `path` without reading it:
    /// loaded (or failed to load), or read on a worker thread since the last ask. When
    /// not, its read is started on a worker thread (once), so a caller can wait a few
    /// frames for it instead of reading it in this frame (#1429).
    pub fn clip_ready(&mut self, path: &str) -> bool {
        let key = fnv_vfs::normalize(path);
        if self.cache.clips.contains_key(&key) {
            return true;
        }
        if let Some(task) = self.cache.clip_reads.get_mut(&key) {
            let Some(read) = check_ready(task) else { return false };
            self.cache.clip_reads.remove(&key);
            self.take_clip(key, read);
            return true;
        }
        let (data, job) = ((*self.models.data).clone(), key.clone());
        let task = crate::read_pool::read_pool().spawn(async move { read_clip(&data, &job) });
        self.cache.clip_reads.insert(key, task);
        false
    }

    /// Keep a clip read (or its failure) under `key`. A KF file the game does not have is
    /// no failure: callers ask for clips an actor may or may not have (a creature folder's
    /// `talking.kf`, its unarmed stance, a weapon's sneak clips), and the base archives
    /// lack most of them too, so it is logged at debug only, not reported as a missing
    /// file (#2277).
    fn take_clip(&mut self, key: String, read: Result<Clip, String>) -> Option<Arc<Clip>> {
        let c = match read {
            Ok(c) => Some(Arc::new(c)),
            Err(e) if e == ABSENT => {
                debug!("{key}: no such clip");
                None
            }
            Err(e) => {
                crate::guard::failed(&mut self.cache.failures, "actor file", format!("{key}: {e}"));
                None
            }
        };
        self.cache.clips.insert(key, c.clone());
        c
    }

    /// The first controller sequence of the KF file at `path` (loaded on first use).
    pub fn clip(&mut self, path: &str) -> Option<Arc<Clip>> {
        let key = fnv_vfs::normalize(path);
        if let Some(c) = self.cache.clips.get(&key) {
            return c.clone();
        }
        // A read started on a worker thread is not started again here; it is dropped.
        self.cache.clip_reads.remove(&key);
        let read = read_clip(&self.models.data, &key);
        self.take_clip(key, read)
    }

    /// The material of shape `i` of the part at `path` with its base texture replaced by
    /// `texture` (a texture path; cached). A skin shape also takes the normal map that
    /// lies beside `texture` ([`skin_normal_map`]).
    pub fn material_with_texture(&mut self, path: &str, part: &PreparedPart, i: usize, texture: &str) -> Handle<NifMaterial> {
        let key = (fnv_vfs::normalize(path), i, fnv_vfs::normalize(texture));
        if let Some(m) = self.cache.materials.get(&key) {
            return m.clone();
        }
        let _span = bevy::log::info_span!("actor::material_with_texture").entered();
        let s = &part.shapes[i];
        let mut props = s.props.clone();
        if props.textures.is_empty() {
            props.textures.push(String::new());
        }
        props.textures[0] = texture.to_string();
        let vfs = &self.models.data.vfs;
        if let Some(normal) = skin_normal_map(s.is_skin(), texture, |p| vfs.exists(p)) {
            if props.textures.len() < 2 {
                props.textures.push(String::new());
            }
            props.textures[1] = normal;
        }
        let mesh_span = bevy::log::info_span!("actor::material_mesh_copy").entered();
        let mut mesh = (*s.cpu).clone();
        drop(mesh_span);
        let m = &mut *self.models;
        let (material, missing) = shape_material_reusing_tangents(&props, &mut mesh, &mut m.textures, &m.data, &mut m.images, m.formats.as_deref());
        let handle = match missing {
            // Keep the model's own texture when the replacement cannot be loaded.
            Some(t) => {
                self.cache.failures.push(format!("{}: texture {t} not loaded", key.0));
                s.material.clone()
            }
            None => self.models.materials.add(material),
        };
        self.cache.materials.insert(key, handle.clone());
        handle
    }

    /// The material of shape `i` of the part at `path` with a texture set's files put in
    /// (a record's alternate textures, texture-swaps.md §3; cached).
    pub fn material_with_texture_set(&mut self, path: &str, part: &PreparedPart, i: usize, set: &[Option<String>; 6]) -> Handle<NifMaterial> {
        let joined: Vec<&str> = set.iter().map(|t| t.as_deref().unwrap_or("")).collect();
        let key = (fnv_vfs::normalize(path), i, format!("txst:{}", joined.join("|").to_ascii_lowercase()));
        if let Some(m) = self.cache.materials.get(&key) {
            return m.clone();
        }
        let _span = bevy::log::info_span!("actor::material_with_texture_set").entered();
        let s = &part.shapes[i];
        let mut props = s.props.clone();
        props.use_texture_set(set);
        let mut mesh = (*s.cpu).clone();
        let m = &mut *self.models;
        let (material, missing) = shape_material_reusing_tangents(&props, &mut mesh, &mut m.textures, &m.data, &mut m.images, m.formats.as_deref());
        let handle = match missing {
            Some(t) => {
                self.cache.failures.push(format!("{}: texture {t} not loaded", key.0));
                s.material.clone()
            }
            None => self.models.materials.add(material),
        };
        self.cache.materials.insert(key, handle.clone());
        handle
    }

    /// A material derived from shape `i`'s, changed by `f` (not cached: per actor).
    pub fn material_variant(&mut self, part: &PreparedPart, i: usize, f: impl FnOnce(&mut NifMaterial)) -> Handle<NifMaterial> {
        let Some(mut m) = self.models.materials.get(&part.shapes[i].material).cloned() else { return part.shapes[i].material.clone() };
        f(&mut m);
        self.models.materials.add(m)
    }

    /// Prepare the part at `path` on this thread, from its file read ahead if there is one.
    fn prepare_part(&mut self, path: &str) -> Result<PreparedPart, String> {
        let ahead = self.cache.fetched.remove(path).map(|(nif, _)| nif);
        let nif = ahead.unwrap_or_else(|| decode(&self.models.data, path));
        let mut sink = self.models.stores();
        prepare_part_into(&mut sink, path, nif, &mut self.cache.failures)
    }
}

/// Prepare the part at `path` from its file `nif` into `sink`: the main thread's stores
/// ([`ActorCtx::part`]) or a worker thread's ([`ActorCtx::parts_ready`], #3072). Textures
/// that could not be loaded are added to `failures`.
fn prepare_part_into(sink: &mut impl ModelSink, path: &str, nif: Nif, failures: &mut Vec<String>) -> Result<PreparedPart, String> {
    let (file, _bytes, dec) = nif?;
    let objects = scene_objects(&file, &dec);
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let shaded = shaded_model(&file, &dec);
    let hidden_self = |b: usize| {
        dec.blocks[b].as_ref().ok().and_then(|s| s.int("flags")).unwrap_or(0) & 1 != 0
            || is_editor_marker(by_block.get(&b).and_then(|o| o.name.as_deref()))
    };
    let mut out = PreparedPart::default();
    let mut rejected = NumericRejections::default();
    // `Prn` on the root node (actors.md §1.2).
    if let Some(r) = objects.iter().find(|o| o.parent.is_none()) {
        out.root = affine(&r.transform);
    }
    if let Some(root) = objects.iter().find(|o| o.parent.is_none()).and_then(|o| dec.blocks[o.block].as_ref().ok()) {
        for &e in root.refs("extra_data").unwrap_or_default() {
            if let Some(x) = dec.get(e).filter(|x| x.text("name").is_some_and(|n| n.eq_ignore_ascii_case("Prn"))) {
                out.prn = x.text("string_data").map(str::to_string);
            }
        }
    }
    if objects.iter().any(|o| o.is_node && o.name.as_deref().is_some_and(is_projectile_node)) {
        let (nodes, _) = crate::model::model_nodes(&objects);
        for (i, n) in nodes.iter().enumerate() {
            let Some(name) = n.name.as_deref().filter(|n| is_projectile_node(n)) else { continue };
            let mut path = Vec::new();
            let mut at = Some(i);
            while let Some(j) = at {
                path.push(j);
                at = nodes[j].parent;
            }
            let local = path.iter().rev().fold(out.root, |m, &j| m * nodes[j].local);
            out.projectile_nodes.push((name.to_string(), local));
        }
    }
    let muzzle_named = |n: &str| MUZZLE_NODES.iter().any(|m| n.eq_ignore_ascii_case(m));
    if objects.iter().any(|o| o.is_node && o.name.as_deref().is_some_and(muzzle_named)) {
        let (nodes, _) = crate::model::model_nodes(&objects);
        out.muzzle = crate::model::node_path(&nodes, &MUZZLE_NODES).map(|path| path.iter().fold(out.root, |m, &j| m * nodes[j].local));
    }
    let mut upb = HashMap::new();
    for o in objects.iter().filter(|o| o.is_node) {
        let (Some(name), Some(Ok(node))) = (&o.name, dec.blocks.get(o.block)) else { continue };
        for &e in node.refs("extra_data").unwrap_or_default() {
            if let Some(x) = dec.get(e).filter(|x| x.text("name").is_some_and(|n| n.eq_ignore_ascii_case("UPB"))) {
                upb.extend(x.text("string_data").map(|s| (name.clone(), s.to_string())));
            }
        }
    }
    out.upb = Arc::new(upb);
    let order = fnv_nif::scene::geometry_order(&objects);
    for o in objects.iter().filter(|o| !o.is_node) {
        // Below the root; the root's own transform is `out.root` (see `rigid_local`).
        let own = if o.parent.is_some() { affine(&o.transform) } else { Affine3A::IDENTITY };
        let mut t = own;
        let mut ancestors = Vec::new();
        let mut hidden = hidden_self(o.block);
        let mut cur = o.parent;
        let mut depth = 0;
        while let Some(p) = cur {
            let Some(po) = by_block.get(&p) else { break };
            if po.parent.is_some() {
                t = affine(&po.transform) * t;
                ancestors.push((po.name.clone(), affine(&po.transform)));
            }
            hidden |= hidden_self(p);
            cur = po.parent;
            depth += 1;
            if depth > 512 {
                break;
            }
        }
        if hidden {
            continue;
        }
        let Ok(shape) = &dec.blocks[o.block] else { continue };
        let Some(data) = shape.block_ref("data").and_then(|r| dec.get(r)) else { continue };
        let mut props = shape_props(&file, &dec, o, &by_block);
        if props.is_helper(shaded) {
            continue;
        }
        // The actor's clips may fade this shape's material alpha (Mr. House's screens).
        props.alpha_gated = crate::nif_spawn::material_alpha_controlled(&file, &dec, o.block);
        let Some(AdmittedShape { mut mesh, skin, .. }) = rejected.admit(o.block, &dec, shape, data) else { continue };
        // Hair-shader shapes (hair, beards, eyebrows) carry a mask in their vertex
        // colours, not a colour [observed: red 0.04–0.39 with green and blue at 1 in
        // `hairbase.nif` and `beardmustachecurly.nif`; multiplied in, the hair turns
        // cyan]; they are not applied [hypothesis: what the mask does is open].
        if props.hair {
            mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
        }
        let cap = is_cap(&dec, shape);
        let mut partitions = shape_partitions(&dec, shape, mesh.count_vertices());
        // In a shape with drawn partitions, only a cap body part (section caps 1xx,
        // torso caps 2xx) with bit 0 clear counts as a cap, so an outfit's piece
        // with an odd flag is never hidden; in a cap shape every partition is one.
        for p in &mut partitions {
            p.cap = partition_is_cap(cap, p.cap, p.body_part);
        }
        let partitions = Arc::new(partitions);
        let caps = partitions.iter().filter(|p| p.cap).count();
        // A cap's texture alpha cuts up to half of its tip away (`MeatCapGore01.dds`,
        // DXT3, tested GREATER 127: dismemberment.md §2.5), and through the cuts the
        // hollow limb and the cap's far side were not drawn: the corpse showed the ground
        // through the stump (owner, 2026-11-15). Caps are drawn from both sides, so the
        // cuts show the cap's inside [design: the caps carry no NiStencilProperty].
        if caps > 0 {
            props.double_sided = true;
        }
        let (material, missing) = shape_material_with(&props, &mut mesh, sink, Tangents::Generate);
        if !rejected.accept_mesh(o.block, &mesh) {
            continue;
        }
        if let Some(t) = missing {
            failures.push(format!("{path}: texture {t} not loaded"));
        }
        if !cap && caps > 0 && first_mixed_report(path, o.block) {
            info!("actor parts: {path}: shape {:?} hides {caps} cap partitions beside {} drawn", o.name.as_deref().unwrap_or(""), partitions.len() - caps);
        }
        let skin = skin.and_then(|skin| {
            let names: Option<Vec<String>> = skin.bones.iter().map(|b| by_block.get(b).and_then(|o| o.name.clone())).collect();
            let names = names?;
            let binds = skin.insert_into(&mut mesh);
            Some((sink.bindposes(binds.into()), names))
        });
        let mesh = if skin.is_some() {
            mesh.with_generated_skinned_mesh_bounds()
                .unwrap_or_else(|_| Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD))
        } else {
            mesh
        };
        let cpu = Arc::new(mesh.clone());
        // A cap draws nothing until its part is gone (`dismember_mesh`); nor does a cap
        // partition in a shape that also holds drawn ones.
        let mesh = if partitions.iter().any(|p| p.cap) { dismember_mesh(&cpu, &partitions, |p| !p.cap) } else { mesh };
        out.shapes.push(PartShape {
            name: o.name.clone(),
            mesh: sink.mesh(mesh),
            material: sink.material(material),
            props,
            cpu,
            skin,
            transform: t,
            cap,
            partitions,
            ancestors,
            own,
            geometry_index: order.get(&o.block).copied().unwrap_or(usize::MAX),
        });
    }
    // Controllers that run by themselves, on a part with no skin: the part moves by
    // itself (Mister Handy's jet, `fxmrhandyjetaddon.nif`: scrolling flames,
    // refraction clouds, billboards; #2228), so a creature draws it as a playing model
    // ([`PreparedPart::plays_itself`]).
    rejected.report(path);
    out.plays_itself = out.shapes.iter().all(|s| s.skin.is_none())
        && (!crate::model::free_material_tracks(&file, &dec).is_empty()
            || !crate::model::free_transform_tracks(&file, &dec).is_empty()
            || !crate::model::free_visibility_tracks(&file, &dec).is_empty());
    Ok(out)
}

/// Read only named BSBound extras attached to skeleton nodes [design, #2535].
fn skeleton_bbx(dec: &Decoded, nodes: &[usize]) -> Option<f32> {
    nodes.iter().filter_map(|&b| dec.blocks.get(b)?.as_ref().ok()).flat_map(|n| n.refs("extra_data").unwrap_or_default()).find_map(|&e| {
        let i = usize::try_from(e).ok()?;
        let bound = dec.get(e).filter(|b| dec.is_a(i, "BSBound") && b.text("name").is_some_and(|n| n.eq_ignore_ascii_case("BBX")))?;
        let dims = bound.floats("dimensions").filter(|d| d.len() == 3)?;
        let x = dims[0];
        (x.is_finite() && x > 0.0).then_some(x)
    })
}

fn prepare_skeleton(nif: Nif) -> Result<PreparedSkeleton, String> {
    let (file, _bytes, dec) = nif?;
    let objects = scene_objects(&file, &dec);
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let order = hierarchy(&objects);
    if order.is_empty() {
        return Err("no root node".into());
    }
    let index: HashMap<usize, usize> = order.iter().enumerate().map(|(i, &b)| (b, i)).collect();
    let mut s = PreparedSkeleton { bbx_half_width: skeleton_bbx(&dec, &order), ..Default::default() };
    for (i, &b) in order.iter().enumerate() {
        let o = by_block.get(&b).ok_or_else(|| format!("block {b} is not a scene object"))?;
        let parent = o.parent.and_then(|p| index.get(&p).copied());
        // A broken skeleton (a mod's file) is refused whole: a NaN bone
        // would spread NaN to every bone below it, the skinned meshes and the physics.
        let t = &o.transform;
        let finite = t.translation.iter().chain(&t.rotation).all(|v| v.is_finite()) && t.scale.is_finite();
        if i > 0 && !finite {
            return Err(format!("node {:?} (block {b}) has a non-finite transform", o.name.as_deref().unwrap_or("")));
        }
        let local = if i == 0 { Affine3A::IDENTITY } else { affine(&o.transform) };
        let world = match parent {
            Some(p) => *s.bind_world.get(p).ok_or_else(|| format!("node {:?} (block {b}) comes before its parent", o.name.as_deref().unwrap_or("")))? * local,
            None => local,
        };
        if let Some(n) = &o.name {
            s.by_name.entry(n.clone()).or_insert(i);
        }
        s.names.push(o.name.clone());
        s.parents.push(parent);
        s.locals.push(if i == 0 { Transform::IDENTITY } else { to_bevy(&o.transform) });
        s.bind_world.push(world);
    }
    Ok(s)
}

/// [design: actors.md §1.2] The skeleton-axis fallback transform of a rigid shape under
/// bone `b`, given the part's root
/// transform and the shape's transform below it: root ∘ shape. On the head bone
/// ([`DEFAULT_RIGID_BONE`]) that is preceded by the inverse of the bone's bind rotation
/// and scale unless the root already is that inverse; on any other bone the part is an
/// ordinary child (actors.md §1.2).
pub fn rigid_local(skel: &PreparedSkeleton, b: usize, root: &Affine3A, shape: &Affine3A) -> Affine3A {
    let head = skel.names.get(b).and_then(|n| n.as_deref()) == Some(DEFAULT_RIGID_BONE);
    // A bone missing or with a bind rotation that cannot be inverted (a broken
    // skeleton): the part is placed as an ordinary child.
    let Some(bind) = skel.bind_world.get(b).map(|w| Mat3::from(w.matrix3)).filter(|m| m.determinant().abs() > 1e-12 && m.is_finite()) else {
        return *root * *shape;
    };
    let inv = bind.inverse();
    let compensated = (Mat3::from(root.matrix3) - inv).to_cols_array().iter().all(|d| d.abs() < 0.02);
    if !head || compensated {
        *root * *shape
    } else {
        Affine3A::from_mat3(inv) * *root * *shape
    }
}

/// [design, #2529] Choose one frame for the part's combined unskinned, non-cap geometry
/// after root and shape transforms. Include hidden shapes so visibility cannot change
/// the frame. A head part centred along bone-local up and spanning skeleton Z=0 keeps
/// its authored frame; otherwise retain `rigid_local`'s rule for the whole part.
/// [hypothesis: actors.md §1.2] This distinguishes bone-local caps from skeleton-axis
/// hats without model names; skinned shapes never enter this attachment path.
fn rigid_part_frame(skel: &PreparedSkeleton, b: usize, part: &PreparedPart) -> Affine3A {
    let fallback = || rigid_local(skel, b, &part.root, &Affine3A::IDENTITY);
    if skel.names.get(b).and_then(|n| n.as_deref()) != Some(DEFAULT_RIGID_BONE) {
        return fallback();
    }
    let Some(bind) = skel.bind_world.get(b).map(|w| Mat3::from(w.matrix3)).filter(|m| m.is_finite() && m.determinant().abs() > 1e-12) else {
        return fallback();
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for shape in part.shapes.iter().filter(|s| s.skin.is_none() && !s.cap) {
        let Some(VertexAttributeValues::Float32x3(vertices)) = shape.cpu.attribute(Mesh::ATTRIBUTE_POSITION) else {
            return fallback();
        };
        let authored = part.root * shape.transform;
        for vertex in vertices {
            let p = authored.transform_point3(Vec3::from_array(*vertex));
            if !p.is_finite() {
                return fallback();
            }
            min = min.min(p);
            max = max.max(p);
        }
    }
    let centre = (min + max) * 0.5;
    let up = (bind.inverse() * Vec3::Z).normalize_or_zero();
    if centre.is_finite() && min.z <= 0.0 && max.z >= 0.0
        && centre.normalize_or_zero().dot(up) >= BONE_LOCAL_HEAD_ALIGNMENT
    {
        return part.root;
    }
    fallback()
}

/// Where an unskinned creature shape hangs (actors.md §10.2): the skeleton node of its
/// nearest ancestor whose name the skeleton has, with the transforms below that
/// ancestor; else the part's `Prn` bone, as an ordinary child with the part's root
/// transform kept (root ∘ shape) whichever bone it is, `Bip01 Head` included: the NPC
/// head rule of [`rigid_local`] does not hold for creatures [observed: dog eyes and
/// skull caps, the Protectron's dome and hat]; else the skeleton's root with the whole
/// transform below the part's root.
pub fn creature_rigid_attach(skel: &PreparedSkeleton, part: &PreparedPart, s: &PartShape) -> (usize, Affine3A) {
    let mut below = s.own;
    for (name, t) in &s.ancestors {
        if let Some(&b) = name.as_ref().and_then(|n| skel.by_name.get(n)) {
            return (b, below);
        }
        below = *t * below;
    }
    match part.prn.as_ref().and_then(|n| skel.by_name.get(n)) {
        Some(&b) => (b, part.root * s.transform),
        None => (0, s.transform),
    }
}

/// [`creature_rigid_attach`] with the part's own named nodes kept as nodes: the bone,
/// the nodes from it down to the shape (outermost first: name, local transform; `None`
/// for the part's root, put in when the shape hangs from `Prn` below a named node, so
/// those nodes keep the locals the part's clips key them by), and the shape's local
/// transform under the last of them (under the bone when there are none). Unnamed nodes
/// are folded into the node or shape below them. The product bone ∘ nodes ∘ shape is
/// [`creature_rigid_attach`]'s placement. A clip names such a node to move it
/// (`SpecialIdle_CutCake.kf` spins `Buzzsawblad`, a node of `gutsybuzzsaw.nif` that the
/// Mister Handy skeleton lacks [observed: `fnvtool nif decode`, #2228]), so the actor
/// needs an entity for it ([`SpawnedActor::part_nodes`]).
pub fn creature_rigid_chain(skel: &PreparedSkeleton, part: &PreparedPart, s: &PartShape) -> (usize, Vec<(Option<String>, Affine3A)>, Affine3A) {
    // Innermost first while walking up: each named node with the transform below it.
    let mut named: Vec<(String, Affine3A)> = Vec::new();
    let mut acc = s.own;
    let mut shape_local = None;
    let mut bone = None;
    for (name, t) in &s.ancestors {
        if let Some(&b) = name.as_ref().and_then(|n| skel.by_name.get(n)) {
            bone = Some(b);
            break;
        }
        match name.as_ref().filter(|n| !n.is_empty()) {
            Some(n) => {
                match named.last_mut() {
                    Some(last) => last.1 = acc,
                    None => shape_local = Some(acc),
                }
                named.push((n.clone(), Affine3A::IDENTITY));
                acc = *t;
            }
            None => acc = *t * acc,
        }
    }
    let mut root_node = None;
    let bone = match bone {
        Some(b) => b,
        None => match part.prn.as_ref().and_then(|n| skel.by_name.get(n)) {
            Some(&b) if named.is_empty() => {
                acc = part.root * acc;
                b
            }
            Some(&b) => {
                root_node = Some(part.root);
                b
            }
            None => 0,
        },
    };
    match named.last_mut() {
        Some(last) => last.1 = acc,
        None => shape_local = Some(acc),
    }
    let mut chain: Vec<(Option<String>, Affine3A)> = root_node.map(|r| (None, r)).into_iter().collect();
    chain.extend(named.into_iter().rev().map(|(n, t)| (Some(n), t)));
    (bone, chain, shape_local.unwrap_or(acc))
}

/// Make a skinned shape that is drawn rigidly (a weapon's or a held object's shape hung
/// from a bone, or a part whose bones the skeleton lacks) a valid skinned draw: a skin of
/// `joints` joints that are all the shape's own entity `e`, with identity inverse bind
/// poses, so each vertex lands where the rigid entity puts it and follows its transform.
/// Its mesh keeps the skin's joint attributes, and Bevy draws such a mesh through the
/// skinned pipeline; with no skin the see-through (blended) pass bound the model-only
/// mesh group to it, a GPU validation error each frame that stopped the run (#1424: the
/// Sierra Madre lobby hologram's boxing tape, drawn see-through by its effect shader)
/// [observed 2026-11-06; bevy_pbr 0.20.1 `SetMeshBindGroup` chooses the blended pass's
/// group by the entity's skin, the pipeline by the mesh's attributes].
pub fn skin_in_place(commands: &mut Commands, e: Entity, joints: usize) {
    let n = joints.max(1);
    commands.queue(move |world: &mut World| {
        let binds = world.resource_mut::<Assets<SkinnedMeshInverseBindposes>>().add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; n]));
        if let Ok(mut ent) = world.get_entity_mut(e) {
            ent.insert(SkinnedMesh { inverse_bindposes: binds, joints: vec![e; n] });
        }
    });
}

/// What [`spawn_actor`] spawned.
#[derive(Debug)]
pub struct SpawnedActor {
    pub root: Entity,
    /// The skeleton's node entities, in [`PreparedSkeleton`] order (`nodes[0]` is `root`).
    pub nodes: Vec<Entity>,
    /// Entities carrying meshes.
    pub drawn: Vec<Entity>,
    /// For each of `drawn`, the index of the part (in the `parts` given) it belongs to.
    pub part_of: Vec<usize>,
    /// For each of `drawn`, the index of its shape in that part's `shapes`.
    pub shape_of: Vec<usize>,
    /// Skinned shapes whose bones the skeleton lacks: drawn rigidly at the root instead.
    pub unbound: usize,
    /// The named nodes of creature parts that the skeleton lacks ([`creature_rigid_chain`]):
    /// name, entity and local transform, for the actor's clips to drive
    /// (`BlendPlayer::add_part_node`).
    pub part_nodes: Vec<(String, Entity, Transform)>,
}

/// Spawn an actor under `parent`: the skeleton's nodes (root at `transform`), then every
/// part's shapes, skinned to the skeleton or hung from their bone. Caps and
/// `hidden_shapes` are not spawned.
pub fn spawn_actor(commands: &mut Commands, parent: Entity, transform: Transform, skel: &PreparedSkeleton, parts: &[PartUse]) -> SpawnedActor {
    let nodes: Vec<Entity> = skel
        .locals
        .iter()
        .enumerate()
        .map(|(i, l)| commands.spawn((if i == 0 { transform } else { *l }, Visibility::Inherited)).id())
        .collect();
    commands.entity(parent).add_child(nodes[0]);
    for (i, p) in skel.parents.iter().enumerate().skip(1) {
        commands.entity(nodes[p.unwrap_or(0)]).add_child(nodes[i]);
    }
    let mut out = SpawnedActor { root: nodes[0], nodes, drawn: Vec::new(), part_of: Vec::new(), shape_of: Vec::new(), unbound: 0, part_nodes: Vec::new() };
    spawn_parts(commands, skel, parts, &mut out);
    out
}

/// On a drawn shape of an actor's part: the shape's name in its model, lower case (what
/// `SwapTexture` names; nvforever `texture_swap.rs`).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct ShapeName(pub String);

/// Spawn every part's shapes on the skeleton nodes of `out` (an actor spawned already:
/// [`spawn_actor`], or parts put on later, as armour taken off shows the body), adding
/// them to its `drawn`, `part_of` (indices into `parts`) and `shape_of`.
/// `cpu` drawn with only the partitions `keep` keeps. A shape without partitions keeps
/// all its triangles when `keep` keeps a BP_TORSO (0) partition that is no cap, else none.
pub fn dismember_mesh(cpu: &Mesh, partitions: &[Partition], keep: impl Fn(&Partition) -> bool) -> Mesh {
    let mut m = cpu.clone();
    let idx: Vec<u32> = if partitions.is_empty() {
        if keep(&Partition { body_part: 0, cap: false, indices: Vec::new() }) {
            return m;
        }
        Vec::new()
    } else {
        partitions.iter().filter(|p| keep(p)).flat_map(|p| p.indices.iter().copied()).collect()
    };
    m.insert_indices(Indices::U32(idx));
    m
}

/// Per vertex: four joint indices and their weights.
pub type SkinWeights<'a> = (&'a [[u16; 4]], &'a [[f32; 4]]);

/// The skin weights of a skinned mesh's vertices.
pub fn skin_weights(m: &Mesh) -> Option<SkinWeights<'_>> {
    match (m.attribute(Mesh::ATTRIBUTE_JOINT_INDEX), m.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)) {
        (Some(VertexAttributeValues::Uint16x4(j)), Some(VertexAttributeValues::Float32x4(w))) if j.len() == w.len() => Some((j, w)),
        _ => None,
    }
}

/// `cpu` cut apart where severed pieces leave the body (docs/spec-notes/dismemberment.md
/// §5): `groups` are triangles (3 indices each) with the side they go with, 0 the body
/// and k > 0 a severed piece, and `side[j]` is the side joint j lies on. Every triangle
/// gets vertices of its own whose weights on joints of other sides are dropped and the
/// rest scaled back up to 1; a vertex left with no weight on its side is carried whole by
/// `fallback[k]`, or keeps its weights when there is none. For the body, a vertex carried
/// only by pieces goes to `above[s]`, the body's joint above the cut of the piece `s`
/// carrying most of it, else to `fallback[0]` (a torso cap weighted to the gone limb's
/// own bones stays at the cut it covers). So no triangle is stretched between a piece and
/// the body.
///
/// `anchors[j]`, for a joint j of a piece, is a joint slot past the shape's own (the
/// caller adds it to the shape's skin) that holds j's place on the body: the bone above
/// the piece's cut, carrying the vertex where j had it. A body vertex keeps its weights on
/// such a joint, moved to its anchor, so the stump, its torso cap and any other body
/// triangle reaching into the piece stay where they were, held by one bone in every shape
/// (#2677: a cap whose shape lacked the stump's bone was carried elsewhere, or off with
/// the piece, and the stump stood open). Joints without an anchor go the old way above.
pub fn sever_mesh(cpu: &Mesh, groups: &[(u8, Vec<u32>)], side: &[u8], fallback: &[Option<u16>], above: &[Option<u16>], anchors: &[Option<u16>]) -> Mesh {
    let mut m = cpu.clone();
    let corner_side: Vec<u8> = groups.iter().flat_map(|(k, t)| std::iter::repeat_n(*k, t.len())).collect();
    m.insert_indices(Indices::U32(groups.iter().flat_map(|(_, t)| t.iter().copied()).collect()));
    m.duplicate_vertices();
    let Some((joints, weights)) = skin_weights(&m) else { return m };
    let (mut joints, mut weights) = (joints.to_vec(), weights.to_vec());
    // An anchor slot lies past the shape's joints, on the body.
    let side_of = |x: u16| side.get(x as usize).copied().unwrap_or(0);
    for ((&k, j), w) in corner_side.iter().zip(&mut joints).zip(&mut weights) {
        let orig = *j;
        if k == 0 {
            for c in 0..4 {
                if w[c] > 0.0 && side_of(j[c]) != 0 {
                    if let Some(a) = anchors.get(j[c] as usize).copied().flatten() {
                        j[c] = a;
                    }
                }
            }
        }
        let mut kept = *w;
        for c in 0..4 {
            if side_of(j[c]) != k {
                kept[c] = 0.0;
            }
        }
        let sum: f32 = kept.iter().sum();
        let carrier = |s: usize| (0..4).filter(|&c| side_of(orig[c]) as usize == s).map(|c| w[c]).sum::<f32>();
        let body_above = || (1..above.len()).filter(|&s| carrier(s) > 0.0).max_by(|&a, &b| carrier(a).total_cmp(&carrier(b))).and_then(|s| above[s]);
        let to = if k == 0 { body_above().or(fallback.first().copied().flatten()) } else { fallback.get(k as usize).copied().flatten() };
        if sum > 1e-4 {
            *w = kept.map(|x| x / sum);
        } else if let Some(f) = to {
            *j = [f, 0, 0, 0];
            *w = [1.0, 0.0, 0.0, 0.0];
        }
    }
    m.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints));
    m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, VertexAttributeValues::Float32x4(weights));
    // Vertices moved to other joints: the per-joint bounds are measured again.
    let _ = m.generate_skinned_mesh_bounds();
    m
}

/// A drawn actor shape, for dismemberment (docs/spec-notes/dismemberment.md §5): what it
/// is drawn from and which bones carry it.
#[derive(Component, Clone)]
pub struct LimbShape {
    pub cap: bool,
    pub partitions: Arc<Vec<Partition>>,
    pub cpu: Arc<Mesh>,
    /// The bones it is skinned to, or the one it hangs from.
    pub bones: Vec<String>,
    /// Its own inverse bind poses (skinned shapes), one per bone: a cut shape is given
    /// these and its anchored copies (`sever_mesh`'s `anchors`).
    pub binds: Option<Handle<SkinnedMeshInverseBindposes>>,
}

pub fn spawn_parts(commands: &mut Commands, skel: &PreparedSkeleton, parts: &[PartUse], out: &mut SpawnedActor) {
    let nodes = out.nodes.clone();
    // The part nodes spawned so far ([`creature_rigid_chain`]), by part and the way down
    // to them, so that a part's shapes under one node share it.
    let mut chains: HashMap<(usize, Vec<Option<String>>), Entity> = HashMap::new();
    for (pi, u) in parts.iter().enumerate() {
        let bone = skel.by_name.get(u.part.attach_bone()).copied();
        let rigid_frame = bone.filter(|_| !u.by_ancestor).map(|b| rigid_part_frame(skel, b, &u.part));
        for (i, s) in u.part.shapes.iter().enumerate() {
            if s.name.as_ref().is_some_and(|n| u.hidden_shapes.iter().any(|h| h.eq_ignore_ascii_case(n))) {
                continue;
            }
            // Caps are spawned with no triangles (`PartShape::cap`); a cap that cannot
            // bind to the skeleton is left out.
            if s.cap && s.skin.as_ref().is_none_or(|(_, names)| names.iter().any(|n| !skel.by_name.contains_key(n))) {
                continue;
            }
            let bones = match &s.skin {
                Some((_, names)) => names.clone(),
                None => vec![u.part.attach_bone().to_string()],
            };
            let limb = LimbShape { cap: s.cap, partitions: s.partitions.clone(), cpu: s.cpu.clone(), bones, binds: s.skin.as_ref().map(|(b, _)| b.clone()) };
            let mesh = u.meshes.get(i).cloned().flatten().unwrap_or_else(|| s.mesh.clone());
            let material = u.materials.get(i).cloned().unwrap_or_else(|| s.material.clone());
            let joints: Option<Vec<Entity>> =
                s.skin.as_ref().and_then(|(_, names)| names.iter().map(|n| skel.by_name.get(n).map(|&j| nodes[j])).collect());
            if matches!((&s.skin, &joints), (Some(_), None)) {
                out.unbound += 1;
            }
            // The shape, and its second layer where it has one (`PartUse::layers`): the
            // same mesh at the same place under the same bones.
            let layer = u.layers.get(i).cloned().flatten();
            for material in std::iter::once(material).chain(layer) {
                let e = match (&s.skin, joints.clone()) {
                    (Some((binds, _)), Some(joints)) => {
                        let e = commands
                            .spawn((
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material),
                                Transform::IDENTITY,
                                SkinnedMesh { inverse_bindposes: binds.clone(), joints },
                                DynamicSkinnedMeshBounds,
                            ))
                            .id();
                        commands.entity(nodes[0]).add_child(e);
                        e
                    }
                    (Some((_, names)), None) => {
                        // Bones missing from the skeleton: draw the bind pose at the root.
                        let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(Mat4::from(s.transform)))).id();
                        commands.entity(nodes[0]).add_child(e);
                        skin_in_place(commands, e, names.len());
                        e
                    }
                    (None, _) => {
                        let (at, local) = match bone {
                            _ if u.by_ancestor => {
                                let (b, chain, local) = creature_rigid_chain(skel, &u.part, s);
                                let mut at = nodes[b];
                                let mut path: Vec<Option<String>> = Vec::with_capacity(chain.len());
                                for (name, t) in chain {
                                    path.push(name.clone());
                                    at = *chains.entry((pi, path.clone())).or_insert_with(|| {
                                        let local = Transform::from_matrix(Mat4::from(t));
                                        let e = commands.spawn((local, Visibility::Inherited, ChildOf(at))).id();
                                        if let Some(n) = name {
                                            out.part_nodes.push((n, e, local));
                                        }
                                        e
                                    });
                                }
                                (at, local)
                            }
                            Some(b) => (nodes[b], rigid_frame.unwrap() * s.transform),
                            None => (nodes[0], s.transform),
                        };
                        let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(Mat4::from(local)))).id();
                        commands.entity(at).add_child(e);
                        e
                    }
                };
                // The shape's name as its model spells it, too: JIP's NIF block functions list
                // an actor's nodes by name (nvforever nif_mirror.rs, #2132).
                if let Some(n) = &s.name {
                    commands.entity(e).insert((ShapeName(n.to_ascii_lowercase()), Name::new(n.clone()), s.stored_look()));
                }
                commands.entity(e).insert(limb.clone());
                out.drawn.push(e);
                out.part_of.push(pi);
                out.shape_of.push(i);
            }
        }
    }
}

/// The skeleton node a clip track named `name` drives: any node but the root, whose
/// transform is the actor's placement.
pub fn clip_target(skel: &PreparedSkeleton, name: &str) -> Option<usize> {
    skel.by_name.get(name).copied().filter(|&i| i > 0)
}

/// The clip's track names split into those that drive a node of `skel` and those that
/// do not (for reports).
pub fn clip_binding<'a>(skel: &PreparedSkeleton, clip: &'a Clip) -> (Vec<&'a str>, Vec<&'a str>) {
    clip.tracks.iter().map(|(n, _)| n.as_str()).partition(|n| clip_target(skel, n).is_some())
}

/// `clip` bound to the nodes of `skel` (the root excluded, [`clip_target`]), to share
/// between the actors of that skeleton.
pub fn bind_clip(skel: &PreparedSkeleton, clip: Arc<Clip>) -> BoundClip {
    BoundClip::bind(clip, |n| clip_target(skel, n))
}

/// A [`BlendPlayer`] for a spawned actor (no clips yet): tracks drive the bone entities
/// by node name, so skinned shapes and rigid parts under their bones follow
/// (docs/spec-notes/actors.md §7 and §8).
pub fn blend_player(actor: &SpawnedActor, skel: &PreparedSkeleton) -> BlendPlayer {
    let mut player = BlendPlayer::new(actor.nodes.clone(), Arc::new(skel.locals.clone()));
    // A creature part's own named nodes, which its clips may name ([`creature_rigid_chain`]).
    for (name, e, local) in &actor.part_nodes {
        player.add_part_node(name, *e, *local);
    }
    player
}

/// The prepared skeleton at `path`, read without a [`ClipReader`] (the examples).
pub fn probe_skeleton(data: &GameData, path: &str) -> Result<PreparedSkeleton, String> {
    prepare_skeleton(decode(data, &fnv_vfs::normalize(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // [design, #2529] Authored fixtures, with no game meshes or copied vertices.
    fn head_shape(vertices: Vec<[f32; 3]>) -> PartShape {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
        PartShape {
            name: Some("fixture cap".into()), mesh: Handle::default(), material: Handle::default(),
            props: Props::default(), cpu: Arc::new(mesh), skin: None,
            transform: Affine3A::IDENTITY, cap: false, partitions: Arc::default(),
            ancestors: Vec::new(), own: Affine3A::IDENTITY, geometry_index: 0,
        }
    }

    fn rotated_head() -> PreparedSkeleton {
        let head = Transform::from_translation(Vec3::new(0.0, 2.0, 100.0))
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
        PreparedSkeleton {
            names: vec![None, Some(DEFAULT_RIGID_BONE.into())],
            parents: vec![None, Some(0)], locals: vec![Transform::IDENTITY, head],
            bind_world: vec![Affine3A::IDENTITY, head.compute_affine()],
            by_name: HashMap::from([(DEFAULT_RIGID_BONE.into(), 1)]),
            bbx_half_width: None,
        }
    }

    #[test]
    fn bone_local_rigid_headgear_follows_spawn_equip_and_head_turns() {
        let skel = rotated_head();
        let rigid = head_shape(vec![[10.0, -2.0, -3.0], [16.0, 2.0, 3.0], [13.0, 0.0, 0.0]]);
        let mut skinned = rigid.clone();
        skinned.skin = Some((Handle::default(), vec![DEFAULT_RIGID_BONE.into()]));
        let part = PartUse {
            part: Arc::new(PreparedPart { shapes: vec![rigid, skinned], ..Default::default() }),
            materials: Vec::new(), meshes: Vec::new(), layers: Vec::new(), hidden_shapes: Vec::new(), by_ancestor: false,
        };
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        let parent = app.world_mut().spawn(Transform::IDENTITY).id();
        let placement = Transform::from_translation(Vec3::new(20.0, -30.0, 4.0))
            .with_rotation(Quat::from_rotation_z(0.7)).with_scale(Vec3::splat(1.2));
        let mut spawned = spawn_actor(&mut app.world_mut().commands(), parent, placement, &skel, std::slice::from_ref(&part));
        app.world_mut().flush();
        app.update();
        let check = |world: &World, e: Entity| {
            assert_eq!(world.get::<ChildOf>(e).unwrap().parent(), spawned.nodes[1]);
            let mesh = world.get::<GlobalTransform>(e).unwrap().affine();
            let head = world.get::<GlobalTransform>(spawned.nodes[1]).unwrap().affine();
            for vertex in [[10.0, -2.0, -3.0], [16.0, 2.0, 3.0], [13.0, 0.0, 0.0]] {
                let v = Vec3::from_array(vertex);
                assert!(mesh.transform_point3(v).distance(head.transform_point3(v)) < 1e-4);
            }
        };
        check(app.world(), spawned.drawn[0]);
        let skin_entity = spawned.drawn[1];
        assert_eq!(app.world().get::<ChildOf>(skin_entity).unwrap().parent(), spawned.root);
        assert_eq!(app.world().get::<SkinnedMesh>(skin_entity).unwrap().joints, vec![spawned.nodes[1]]);
        assert_eq!(*app.world().get::<Transform>(skin_entity).unwrap(), Transform::IDENTITY);
        let initial = app.world().get::<GlobalTransform>(spawned.drawn[0]).unwrap().transform_point(Vec3::X * 13.0);
        app.world_mut().get_mut::<Transform>(spawned.nodes[1]).unwrap().rotation *= Quat::from_rotation_z(0.6);
        app.update();
        check(app.world(), spawned.drawn[0]);
        let turned = app.world().get::<GlobalTransform>(spawned.drawn[0]).unwrap().transform_point(Vec3::X * 13.0);
        assert!(initial.distance(turned) > 1.0);
        // [design] Equipment added after the head turns takes the same attachment path.
        spawn_parts(&mut app.world_mut().commands(), &skel, &[part], &mut spawned);
        app.world_mut().flush();
        app.update();
        let equipped = *spawned.drawn.iter().rev().nth(1).unwrap();
        let mesh = app.world().get::<GlobalTransform>(equipped).unwrap().affine();
        let head = app.world().get::<GlobalTransform>(spawned.nodes[1]).unwrap().affine();
        assert!(mesh.transform_point3(Vec3::X * 13.0).distance(head.transform_point3(Vec3::X * 13.0)) < 1e-4);
    }

    #[test]
    fn rigid_headgear_material_submeshes_share_the_part_frame() {
        let skel = rotated_head();
        let cap_vertices = vec![[10.0, -2.0, -3.0], [16.0, 2.0, 3.0], [13.0, 0.0, 0.0]];
        let badge_vertices = vec![[10.0, 0.0, 2.0], [16.0, 0.0, 4.0], [13.0, 0.0, 3.0]];
        let cap_transform = Affine3A::from_rotation_translation(Quat::from_rotation_x(0.3), Vec3::new(1.0, -0.5, 0.5));
        let badge_transform = Affine3A::from_rotation_translation(Quat::from_rotation_y(-0.2), Vec3::new(2.0, 0.0, 1.0));
        // [design, #2529] Identical authored geometry, split into transformed material
        // submeshes or joined into one shape. The badge alone does not span Z=0.
        let transformed_shape = |vertices: &[[f32; 3]], transform: Affine3A| {
            let mut shape = head_shape(vertices.iter().map(|v| transform.inverse().transform_point3(Vec3::from_array(*v)).to_array()).collect());
            shape.transform = transform;
            shape
        };
        let cap = transformed_shape(&cap_vertices, cap_transform);
        let badge = transformed_shape(&badge_vertices, badge_transform);
        let joined = head_shape(cap_vertices.into_iter().chain(badge_vertices).collect());
        for root in [Affine3A::IDENTITY, Affine3A::from_scale_rotation_translation(Vec3::splat(1.1), Quat::from_rotation_z(0.2), Vec3::X * 0.4)] {
            let part_use = |shapes| PartUse {
                part: Arc::new(PreparedPart { shapes, root, ..Default::default() }),
                materials: Vec::new(), meshes: Vec::new(), layers: Vec::new(), hidden_shapes: Vec::new(), by_ancestor: false,
            };
            let parts = [part_use(vec![cap.clone(), badge.clone()]), part_use(vec![joined.clone()])];
            let mut app = App::new();
            app.add_plugins(bevy::transform::TransformPlugin);
            let parent = app.world_mut().spawn(Transform::IDENTITY).id();
            let placement = Transform::from_translation(Vec3::new(20.0, -30.0, 4.0))
                .with_rotation(Quat::from_rotation_z(0.7)).with_scale(Vec3::splat(1.2));
            let mut spawned = spawn_actor(&mut app.world_mut().commands(), parent, placement, &skel, &parts);
            app.world_mut().flush();
            app.update();
            let check = |world: &World, spawned: &SpawnedActor, drawn: &[Entity]| {
                let head = world.get::<GlobalTransform>(spawned.nodes[1]).unwrap().affine();
                let combined = world.get::<GlobalTransform>(drawn[2]).unwrap().affine();
                for (entity, shape) in drawn[..2].iter().zip([&cap, &badge]) {
                    assert_eq!(world.get::<ChildOf>(*entity).unwrap().parent(), spawned.nodes[1]);
                    let mesh = world.get::<GlobalTransform>(*entity).unwrap().affine();
                    let VertexAttributeValues::Float32x3(vertices) = shape.cpu.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!() };
                    for vertex in vertices {
                        let v = Vec3::from_array(*vertex);
                        let authored = shape.transform.transform_point3(v);
                        let actual = mesh.transform_point3(v);
                        assert!(actual.distance((head * root).transform_point3(authored)) < 1e-4);
                        assert!(actual.distance(combined.transform_point3(authored)) < 1e-4);
                    }
                }
            };
            check(app.world(), &spawned, &spawned.drawn);
            app.world_mut().get_mut::<Transform>(spawned.nodes[1]).unwrap().rotation *= Quat::from_rotation_z(0.6);
            app.update();
            check(app.world(), &spawned, &spawned.drawn);
            spawn_parts(&mut app.world_mut().commands(), &skel, &parts, &mut spawned);
            app.world_mut().flush();
            app.update();
            check(app.world(), &spawned, &spawned.drawn[3..]);
        }
    }

    #[test]
    fn rigid_head_frame_detection_keeps_existing_layouts_and_authored_nodes() {
        let skel = rotated_head();
        let z_hat = head_shape(vec![[-3.0, -2.0, 8.0], [3.0, 2.0, 16.0]]);
        let x_cap = head_shape(vec![[10.0, -2.0, -3.0], [16.0, 2.0, 3.0]]);
        let local = |skel: &PreparedSkeleton, b, root: &Affine3A, s: &PartShape| {
            let part = PreparedPart { root: *root, shapes: vec![s.clone()], ..Default::default() };
            rigid_part_frame(skel, b, &part) * s.transform
        };
        let old = |root: &Affine3A, s: &PartShape| rigid_local(&skel, 1, root, &s.transform);
        assert_eq!(local(&skel, 1, &Affine3A::IDENTITY, &z_hat), old(&Affine3A::IDENTITY, &z_hat));
        let inverse = Affine3A::from_mat3(Mat3::from(skel.bind_world[1].matrix3).inverse());
        assert_eq!(local(&skel, 1, &inverse, &z_hat), inverse);
        let root = Affine3A::from_rotation_z(0.4);
        assert_eq!(local(&skel, 1, &root, &x_cap), root);
        let mut cancelling = z_hat.clone();
        cancelling.transform = root.inverse();
        assert_eq!(local(&skel, 1, &root, &cancelling), old(&root, &cancelling));
        let mut offset = x_cap.clone();
        offset.transform = Affine3A::from_translation(Vec3::X);
        assert_eq!(local(&skel, 1, &Affine3A::IDENTITY, &offset), offset.transform);
        for vertices in [vec![], vec![[-1.0; 3], [1.0; 3]], vec![[f32::NAN, 0.0, 0.0]], vec![[10.0, 0.0, 2.0], [16.0, 0.0, 4.0]]] {
            let s = head_shape(vertices);
            assert_eq!(local(&skel, 1, &Affine3A::IDENTITY, &s), old(&Affine3A::IDENTITY, &s));
        }
        let mut off_head = skel.clone();
        off_head.names[1] = Some("fixture forearm".into());
        assert_eq!(local(&off_head, 1, &root, &x_cap), root);
        let mut broken = skel.clone();
        broken.bind_world[1] = Affine3A::from_mat3(Mat3::ZERO);
        assert_eq!(local(&broken, 1, &root, &x_cap), root);
    }

    #[test]
    fn rigid_part_frame_ignores_skinned_and_cap_geometry_but_rejects_malformed_siblings() {
        let skel = rotated_head();
        let cap = head_shape(vec![[10.0, -2.0, -3.0], [16.0, 2.0, 3.0]]);
        let mut skinned = head_shape(vec![[0.0, 0.0, -1000.0]]);
        skinned.skin = Some((Handle::default(), vec![DEFAULT_RIGID_BONE.into()]));
        let mut dismemberment_cap = head_shape(vec![[0.0, 0.0, 1000.0]]);
        dismemberment_cap.cap = true;
        let mut part = PreparedPart { shapes: vec![cap, skinned, dismemberment_cap], ..Default::default() };
        assert_eq!(rigid_part_frame(&skel, 1, &part), Affine3A::IDENTITY);
        part.shapes.push(head_shape(vec![[f32::NAN, 0.0, 0.0]]));
        let fallback = rigid_local(&skel, 1, &part.root, &Affine3A::IDENTITY);
        assert_eq!(rigid_part_frame(&skel, 1, &part), fallback);
        Arc::make_mut(&mut part.shapes[3].cpu).remove_attribute(Mesh::ATTRIBUTE_POSITION);
        assert_eq!(rigid_part_frame(&skel, 1, &part), fallback);
    }

    #[test]
    fn skeleton_bbx_reads_only_attached_named_bounds() {
        use fnv_nif::value::Struct;
        // Synthetic decoded blocks, without game assets [design, #2535].
        let read = |name: &str, ty: &str, dims: Vec<f32>, attached: bool| {
            let node = Struct { fields: vec![("extra_data".into(), Value::RefList(if attached { vec![-1, 1] } else { vec![] }))] };
            let bound = Struct { fields: vec![
                ("name".into(), Value::Str { index: None, text: Some(name.into()) }),
                ("dimensions".into(), Value::Floats(dims)),
            ] };
            let dec = Decoded { blocks: vec![Ok(node), Ok(bound)], lineage: vec![vec!["NiNode".into()], vec![ty.into()]] };
            skeleton_bbx(&dec, &[0])
        };
        assert_eq!(read("BBX", "BSBound", vec![80.0, 120.0, 40.0], true), Some(80.0));
        assert_eq!(read("bbx", "BSBound", vec![35.0, 40.0, 20.0], true), Some(35.0));
        assert_eq!(read("Other", "BSBound", vec![80.0; 3], true), None);
        assert_eq!(read("BBX", "NiStringExtraData", vec![80.0; 3], true), None);
        assert_eq!(read("BBX", "BSBound", vec![80.0; 3], false), None);
        for x in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(read("BBX", "BSBound", vec![x, 40.0, 20.0], true), None);
        }
        assert_eq!(read("BBX", "BSBound", vec![80.0], true), None);
    }

    #[test]
    fn a_severed_piece_takes_its_own_vertices_weighted_to_its_side_only() {
        // Two triangles sharing vertices 1 and 2, which are weighted half to joint 0 (the
        // body) and half to joint 1 (the piece); vertex 3 only to the body.
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 4]);
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0], [0, 1, 0, 0], [0, 1, 0, 0], [0, 0, 0, 0]]));
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1.0f32, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
        let out = sever_mesh(&m, &[(0, vec![0, 1, 2]), (1, vec![1, 2, 3])], &[0, 1], &[None, Some(1)], &[], &[]);
        let (j, w) = skin_weights(&out).unwrap();
        assert_eq!(out.count_vertices(), 6);
        // The body's copies: all on joint 0.
        assert!(j[..3].iter().zip(&w[..3]).all(|(j, w)| j[0] == 0 && w[0] == 1.0));
        // The piece's copies of 1 and 2: all on joint 1; vertex 3 had nothing there, so
        // the fallback carries it.
        assert_eq!((j[3], w[3]), ([0, 1, 0, 0], [0.0, 1.0, 0.0, 0.0]));
        assert_eq!((j[5], w[5]), ([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn a_torso_cap_carried_by_the_gone_limb_stays_on_the_body_bone_above_its_cut() {
        // Joints: 0 the pelvis, 1 the neck (body); 2 the right thigh (piece 1), 3 the head
        // (piece 2). Like `republican_02.nif`'s torso cap 210, weighted ~92 % to the right
        // thigh: vertex 0 all on the thigh, 1 mostly on it, 2 all on the head.
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[2, 0, 0, 0], [2, 3, 0, 0], [3, 0, 0, 0]]));
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1.0f32, 0.0, 0.0, 0.0], [0.7, 0.3, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
        let side = [0, 0, 1, 2];
        // The body's triangle: the thigh's vertices go to the pelvis (above piece 1's
        // cut), the head's to the neck (above piece 2's), not all to one bone.
        let out = sever_mesh(&m, &[(0, vec![0, 1, 2]), (1, vec![]), (2, vec![])], &side, &[Some(1), Some(2), Some(3)], &[None, Some(0), Some(1)], &[]);
        let (j, w) = skin_weights(&out).unwrap();
        assert_eq!((j[0], w[0]), ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        assert_eq!((j[1], w[1]), ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        assert_eq!((j[2], w[2]), ([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        // Without a bone above a piece's cut, the body's fallback carries the vertex.
        let out = sever_mesh(&m, &[(0, vec![0, 1, 2])], &side, &[Some(1)], &[None, None, None], &[]);
        let (j, _) = skin_weights(&out).unwrap();
        assert!(j.iter().all(|j| j[0] == 1));
    }

    #[test]
    fn a_body_vertex_on_a_gone_joint_keeps_its_weights_on_that_joints_anchor() {
        // Joints: 0 the pelvis (body), 1 the thigh and 2 the calf (piece 1); anchors 3 and
        // 4 hold the thigh's and the calf's places on the body. Vertex 0 half on the pelvis
        // and half on the thigh, 1 on the thigh and the calf, 2 on the pelvis.
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0, 1, 0, 0], [1, 2, 0, 0], [0, 0, 0, 0]]));
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[0.5f32, 0.5, 0.0, 0.0], [0.6, 0.4, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
        let side = [0, 1, 1];
        let anchors = [None, Some(3), Some(4)];
        let out = sever_mesh(&m, &[(0, vec![0, 1, 2]), (1, vec![0, 1, 2])], &side, &[Some(0), Some(1)], &[None, Some(0)], &anchors);
        let (j, w) = skin_weights(&out).unwrap();
        // The body's copies keep their weights, the piece's joints moved to their anchors.
        assert_eq!((j[0], w[0]), ([0, 3, 0, 0], [0.5, 0.5, 0.0, 0.0]));
        assert_eq!((j[1], w[1]), ([3, 4, 0, 0], [0.6, 0.4, 0.0, 0.0]));
        assert_eq!((j[2], w[2]), ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        // The piece's copies are untouched by the anchors: only its own joints carry them.
        assert_eq!((j[3], w[3]), ([0, 1, 0, 0], [0.0, 1.0, 0.0, 0.0]));
        assert_eq!((j[4], w[4]), ([1, 2, 0, 0], [0.6, 0.4, 0.0, 0.0]));
        assert_eq!((j[5], w[5]), ([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn cap_partitions_are_hidden_also_in_a_shape_with_drawn_ones() {
        // The radscorpion's `meatcapbody` (dismemberment.md §2.5): torso caps 206 and 203
        // (part flag bit 0 clear) around one editor-visible BP_TORSO triangle.
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 9]);
        let parts = [
            Partition { body_part: 206, cap: true, indices: vec![0, 1, 2] },
            Partition { body_part: 0, cap: false, indices: vec![3, 4, 5] },
            Partition { body_part: 203, cap: true, indices: vec![6, 7, 8] },
        ];
        let intact = dismember_mesh(&m, &parts, |p| !p.cap);
        assert_eq!(intact.indices().unwrap().iter().collect::<Vec<_>>(), [3, 4, 5]);
        // Its left foreleg (3) gone: that leg's torso cap shows too.
        let gone = dismember_mesh(&m, &parts, |p| !p.cap || p.body_part == 203);
        assert_eq!(gone.indices().unwrap().iter().collect::<Vec<_>>(), [3, 4, 5, 6, 7, 8]);
        // Which partitions count as caps: a flag-clear torso cap beside drawn ones; not
        // the flag-clear 65535 of `wastelandchild02\outfitf.nif`'s `Arms`; all of a cap shape.
        assert!(partition_is_cap(false, true, 205) && partition_is_cap(false, true, 101));
        assert!(!partition_is_cap(false, true, 65535) && !partition_is_cap(false, false, 205));
        assert!(partition_is_cap(true, true, 0));
    }

    #[test]
    fn a_part_texture_goes_on_the_skin_shapes_when_there_are_any() {
        // upperbody.nif: underwear, skin, caps.
        assert_eq!(texture_targets(&[(false, false), (false, true), (true, false)]), [false, true, false]);
        // An eye or a hair model (NoHat, Hat): no skin shape, so every shape.
        assert_eq!(texture_targets(&[(false, false), (false, false)]), [true, true]);
        // A cap of the skin type does not make the part a skin part.
        assert_eq!(texture_targets(&[(false, false), (true, true)]), [true, false]);
        assert_eq!(texture_targets(&[]), Vec::<bool>::new());
    }

    #[test]
    fn a_skin_shape_takes_the_normal_map_beside_its_new_texture() {
        let archive = ["textures\\characters\\ghoul\\upperbodymale_n.dds", "textures\\characters\\mouth\\mouthhuman_n.dds"];
        let exists = |p: &str| archive.contains(&p);
        // As an ICON gives it (no `textures\` in front, mixed case): found by its VFS path,
        // returned as written.
        assert_eq!(skin_normal_map(true, "Characters\\Ghoul\\UpperBodyMale.dds", exists).as_deref(), Some("Characters\\Ghoul\\UpperBodyMale_n.dds"));
        // No such file: the model's own normal map stays.
        assert_eq!(skin_normal_map(true, "characters\\ghoul\\handmale.dds", exists), None);
        // Not a skin shape (mouth, eyes, hair): the model's own stays even when the file exists.
        assert_eq!(skin_normal_map(false, "characters\\mouth\\mouthhuman.dds", exists), None);
        assert_eq!(skin_normal_map(true, "", exists), None);
    }

    #[test]
    fn creature_rigid_shapes_hang_from_their_nearest_skeleton_ancestor() {
        let skel = PreparedSkeleton {
            by_name: HashMap::from([("Bip01 Head".to_string(), 3)]),
            bind_world: vec![Affine3A::IDENTITY; 4],
            ..Default::default()
        };
        let screen = Affine3A::from_translation(Vec3::new(0.0, 5.0, 0.0));
        let head_copy = Affine3A::from_translation(Vec3::new(0.0, 0.0, 100.0));
        let own = Affine3A::from_translation(Vec3::new(1.0, 0.0, 0.0));
        let s = |ancestors: Vec<(Option<String>, Affine3A)>| PartShape {
            name: None,
            mesh: Handle::default(),
            material: Handle::default(),
            props: Props::default(),
            cpu: Arc::new(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)),
            skin: None,
            transform: ancestors.iter().rev().fold(Affine3A::IDENTITY, |acc, a| acc * a.1) * own,
            cap: false,
            partitions: Arc::default(),
            ancestors,
            own,
            geometry_index: 0,
        };
        let mut part = PreparedPart::default();
        // Screen01 (not a bone) under the part's copy of Bip01 Head: hangs from the head
        // with Screen01's transform kept.
        let shape = s(vec![(Some("Screen01".into()), screen), (Some("Bip01 Head".into()), head_copy)]);
        let (b, local) = creature_rigid_attach(&skel, &part, &shape);
        assert_eq!(b, 3);
        assert!((Vec3::from(local.translation) - Vec3::new(1.0, 5.0, 0.0)).length() < 1e-5);
        // No ancestor the skeleton has, no Prn: the root, with the whole transform.
        let shape = s(vec![(Some("Screen01".into()), screen)]);
        let (b, local) = creature_rigid_attach(&skel, &part, &shape);
        assert_eq!(b, 0);
        assert!((Vec3::from(local.translation) - Vec3::new(1.0, 5.0, 0.0)).length() < 1e-5);
        // No such ancestor but a `Prn`: that bone, with the part's root transform kept.
        part.prn = Some("Bip01 Head".into());
        part.root = Affine3A::from_translation(Vec3::new(0.0, 0.0, 7.0));
        let (b, local) = creature_rigid_attach(&skel, &part, &shape);
        assert_eq!(b, 3);
        assert!((Vec3::from(local.translation) - Vec3::new(1.0, 5.0, 7.0)).length() < 1e-5);
    }

    #[test]
    fn creature_part_nodes_are_kept_and_place_shapes_as_before() {
        let skel = PreparedSkeleton {
            by_name: HashMap::from([("Bip01 R Foot 01".to_string(), 2), ("Bip01 Head".to_string(), 3)]),
            bind_world: vec![Affine3A::IDENTITY; 4],
            ..Default::default()
        };
        let blade = Affine3A::from_rotation_translation(Quat::from_rotation_x(1.0), Vec3::new(15.9, -0.7, 0.1));
        let spacer = Affine3A::from_translation(Vec3::new(0.0, 2.0, 0.0));
        let own = Affine3A::from_translation(Vec3::new(21.8, 10.8, -3.6));
        let shape = |ancestors: Vec<(Option<String>, Affine3A)>| PartShape {
            name: None,
            mesh: Handle::default(),
            material: Handle::default(),
            props: Props::default(),
            cpu: Arc::new(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)),
            skin: None,
            transform: ancestors.iter().rev().fold(Affine3A::IDENTITY, |acc, a| acc * a.1) * own,
            cap: false,
            partitions: Arc::default(),
            ancestors,
            own,
            geometry_index: 0,
        };
        let product = |b: usize, chain: &[(Option<String>, Affine3A)], local: Affine3A| (b, chain.iter().fold(Affine3A::IDENTITY, |acc, (_, t)| acc * *t) * local);
        let close = |a: Affine3A, b: Affine3A| Mat4::from(a).abs_diff_eq(Mat4::from(b), 1e-4);
        // `gutsybuzzsaw.nif`: the blade's shape under `Buzzsawblad` under the root, `Prn`
        // the foot. A root of its own when it is not the identity, then the blade node with
        // its own transform (what the clip's keys replace), then the shape.
        let mut part = PreparedPart { prn: Some("Bip01 R Foot 01".into()), root: Affine3A::from_translation(Vec3::new(0.0, 0.0, 3.0)), ..Default::default() };
        let s = shape(vec![(None, spacer), (Some("Buzzsawblad".into()), blade)]);
        let (b, chain, local) = creature_rigid_chain(&skel, &part, &s);
        assert_eq!(b, 2);
        assert_eq!(chain.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(), vec![None, Some("Buzzsawblad".to_string())]);
        assert!(close(chain[1].1, blade));
        assert!(close(local, spacer * own));
        let (b0, old) = creature_rigid_attach(&skel, &part, &s);
        let (b1, new) = product(b, &chain, local);
        assert_eq!(b0, b1);
        assert!(close(old, new));
        // With nothing named below the root: no nodes, the old placement.
        part.root = Affine3A::IDENTITY;
        let s = shape(vec![(None, spacer)]);
        let (b, chain, local) = creature_rigid_chain(&skel, &part, &s);
        assert!(chain.is_empty());
        assert_eq!((b, local), creature_rigid_attach(&skel, &part, &s));
        // A named node below an ancestor the skeleton has: kept, under that bone.
        let s = shape(vec![(Some("Screen01".into()), spacer), (Some("Bip01 Head".into()), blade)]);
        let (b, chain, local) = creature_rigid_chain(&skel, &part, &s);
        assert_eq!(b, 3);
        assert_eq!(chain.len(), 1);
        let (b0, old) = creature_rigid_attach(&skel, &part, &s);
        let (b1, new) = product(b, &chain, local);
        assert_eq!(b0, b1);
        assert!(close(old, new));
    }

    #[test]
    fn creature_parts_on_the_head_are_plain_children() {
        // A dog's head bone: its x axis runs up and forward along the skull, its z axis
        // to the dog's left (-x of the skeleton; the dog faces +y). The eye model's root
        // is a half turn about an axis in the bone's xy plane; it is neither the identity
        // nor the inverse of the bone's rotation, so the NPC head rule would put both
        // eyes on one side of the head (actors.md §10.2).
        let (c, s) = (0.4, 0.9165);
        let rot = Mat3::from_cols(Vec3::new(0.0, c, s), Vec3::new(0.0, s, -c), -Vec3::X);
        let bone = Affine3A::from_mat3_translation(rot, Vec3::new(0.0, 42.1, 56.7));
        let skel = PreparedSkeleton {
            by_name: HashMap::from([(DEFAULT_RIGID_BONE.to_string(), 1)]),
            bind_world: vec![Affine3A::IDENTITY, bone],
            names: vec![None, Some(DEFAULT_RIGID_BONE.to_string())],
            ..Default::default()
        };
        let root = Affine3A::from_mat3(Mat3::from_cols(Vec3::new(c, s, 0.0), Vec3::new(s, -c, 0.0), -Vec3::Z));
        let part = PreparedPart { prn: Some(DEFAULT_RIGID_BONE.to_string()), root, ..Default::default() };
        let shape = PartShape {
            name: None,
            mesh: Handle::default(),
            material: Handle::default(),
            props: Props::default(),
            cpu: Arc::new(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)),
            skin: None,
            transform: Affine3A::IDENTITY,
            cap: false,
            partitions: Arc::default(),
            ancestors: Vec::new(),
            own: Affine3A::IDENTITY,
            geometry_index: 0,
        };
        let (b, local) = creature_rigid_attach(&skel, &part, &shape);
        assert_eq!(b, 1);
        assert_eq!(local, root);
        // The two eyes of the model (±z) land either side of the skeleton's x = 0, at
        // the same height and the same distance forward.
        let eye = |z: f32| (bone * local).transform_point3(Vec3::new(10.8, 2.9, z));
        let (l, r) = (eye(3.5), eye(-3.5));
        assert!((l.x + r.x).abs() < 1e-3 && (l.x - r.x).abs() > 6.9, "{l} {r}");
        assert!((l.y - r.y).abs() < 1e-3 && (l.z - r.z).abs() < 1e-3, "{l} {r}");
        // The NPC head rule on the same data: one eye above the other, off to one side.
        let npc = bone * rigid_local(&skel, 1, &root, &Affine3A::IDENTITY);
        let (l, r) = (npc.transform_point3(Vec3::new(10.8, 2.9, 3.5)), npc.transform_point3(Vec3::new(10.8, 2.9, -3.5)));
        assert!((l.x - r.x).abs() < 1e-3 && (l.z - r.z).abs() > 6.9, "{l} {r}");
    }

    #[test]
    fn rigid_parts_keep_skeleton_axes() {
        // A bone whose x axis points up (as `Bip01 Head`), at height 112.8.
        let rot = Mat3::from_cols(Vec3::Z, Vec3::Y, -Vec3::X);
        let bone = Affine3A::from_mat3_translation(rot, Vec3::new(0.0, 0.4, 112.8));
        let skel = PreparedSkeleton {
            bind_world: vec![Affine3A::IDENTITY, bone],
            names: vec![None, Some(DEFAULT_RIGID_BONE.to_string())],
            ..Default::default()
        };
        let up = Vec3::new(0.0, 0.0, 15.0);
        let above = Vec3::new(0.0, 0.4, 127.8);
        // Identity nodes (hats, beards): the attachment compensates the bone's rotation.
        let p = (bone * rigid_local(&skel, 1, &Affine3A::IDENTITY, &Affine3A::IDENTITY)).transform_point3(up);
        assert!((p - above).length() < 1e-4, "{p}");
        // Node rotations that cancel (hair): the same.
        let r0 = Affine3A::from_mat3(Mat3::from_cols(-Vec3::X, Vec3::Z, Vec3::Y));
        let p = (bone * rigid_local(&skel, 1, &r0, &r0)).transform_point3(up);
        assert!((p - above).length() < 1e-4, "{p}");
        // A root that stores the bone's inverse rotation (eyes): an ordinary child.
        let inv = Affine3A::from_mat3(rot.inverse());
        let p = (bone * rigid_local(&skel, 1, &inv, &Affine3A::IDENTITY)).transform_point3(up);
        assert!((p - above).length() < 1e-4, "{p}");
    }

    #[test]
    fn rigid_parts_off_the_head_are_plain_children() {
        // A forearm bone whose x axis runs down the arm (as `Bip01 L ForeTwist`): a part
        // modelled 10 units along x (the Pip-Boy) sits 10 units down the arm, whatever
        // the part's node transforms are.
        let rot = Mat3::from_cols(-Vec3::X, -Vec3::Y, Vec3::Z);
        let bone = Affine3A::from_mat3_translation(rot, Vec3::new(-29.0, -3.7, 94.4));
        let skel = PreparedSkeleton {
            bind_world: vec![Affine3A::IDENTITY, bone],
            names: vec![None, Some("Bip01 L ForeTwist".to_string())],
            ..Default::default()
        };
        let shape = Affine3A::from_translation(Vec3::new(8.0, 0.0, 0.0));
        let local = rigid_local(&skel, 1, &Affine3A::IDENTITY, &shape);
        assert_eq!(local, shape);
        let p = (bone * local).transform_point3(Vec3::new(2.0, 0.0, 0.0));
        assert!((p - Vec3::new(-39.0, -3.7, 94.4)).length() < 1e-4, "{p}");
    }

    #[test]
    fn a_degenerate_head_bone_places_parts_as_plain_children() {
        let skel = PreparedSkeleton {
            bind_world: vec![Affine3A::IDENTITY, Affine3A::from_mat3(Mat3::ZERO)],
            names: vec![None, Some(DEFAULT_RIGID_BONE.to_string())],
            ..Default::default()
        };
        let shape = Affine3A::from_translation(Vec3::new(8.0, 0.0, 0.0));
        assert_eq!(rigid_local(&skel, 1, &Affine3A::IDENTITY, &shape), shape);
        // A bone the skeleton does not have: the same.
        assert_eq!(rigid_local(&skel, 9, &Affine3A::IDENTITY, &shape), shape);
    }
}
