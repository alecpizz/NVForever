//! Solid cores of blended shapes under the light in the air (#3032): an engineering choice of
//! NVForever's own for the enhanced look [design], never a parity item and never used to
//! explain a difference from the original; nothing of it runs while the light in the air is
//! not drawn ([`AirDraws`]: the row or the aid asks for it and its gate is open; at night without a moon, indoors,
//! under water and at a strength of 0 there are none; in the half second the gate takes to settle they
//! are hidden, from the frame the composite stops).
//!
//! **Why.** The light's composite (medium_air_compose.rs) is a fullscreen pass over the
//! finished picture that tells each pixel's distance from the depth prepass. Bevy draws no
//! blended material into that prepass, so a shape whose NiAlphaProperty blends but that is
//! solid to the eye was painted with the light of what lies behind it (the water
//! tank by Goodsprings, `nv_watertank.nif`, flags 4333, reference 00106B5E:
//! docs/design/volumetric-medium.md, "Solid cores of blended shapes").
//!
//! **What.** While [`AirDraws`] is true, a placed shape whose material can have one
//! (`nif_spawn::solid_core_rules` at the material's build, `nif_spawn::solid_core` of the
//! material as it stands now, and a base texture with a solid texel,
//! `texture::alpha_solidity`) gets a second draw, its solid core: a loose entity (nobody's
//! child) in the shape's place with the same mesh and a copy of its material as a cut-out at
//! `nif_spawn::SOLID_CORE_CUT`. The core is in the depth prepass; the blended draw goes on as
//! it was, over the core at equal depth (greater or equal passes), so the soft edges stay.
//! A core casts no shadow (`NotShadowCaster`): shadows are as without it.
//!
//! **Following its shape.** The link is kept here alone: in [`SolidCores`] by the shape, and on
//! the core ([`SolidCore::shape`]). The shape itself gains nothing, no component and no child, so
//! nothing that walks or queries the reference's entities can tell whether the light is on
//! (its archetype, its children, the scripts' NIF nodes, the reference's root). Every frame,
//! after the transforms and the visibility are propagated and before the visibility check,
//! [`place`] gives each core its shape's `GlobalTransform` and `InheritedVisibility`, bit for
//! bit. The core has no `Transform` and no `Visibility` of its own: from a `Transform` the
//! propagation would work its `GlobalTransform` out anew, and a matrix taken apart and put
//! together again (a rotated, scaled or sheared one) is not the same bits, so the two draws
//! would not lie at equal depth. The motion vectors need neither: Bevy's
//! `PreviousGlobalTransform` is the `GlobalTransform` of the frame before (PreUpdate),
//! and a new core starts with its
//! shape's (the CPU path; the GPU's instance history is kept by entity, so there a new core has no
//! motion for one frame).
//! A core ends with its shape (its `Mesh3d` gone: cell unload, travel, load, a script's
//! removal) in the same frame, and with the light; one that nothing is known of any more is
//! taken away by [`place`]. Render layers (room culling puts them on and takes them off), shadow receiving and the
//! fades' visibility range are copied when the shape's change, removals included; another mesh
//! or another core material makes the core anew (its pipelines are chosen when it is made, and
//! the frame's check for changed ones may be over). There is
//! one core material per source material, dropped with its last user.
//!
//! **Fades and edits.** A core must never stay solid under a thing that fades or whose outline
//! moves. Every fade, script alpha, stealth field, effect-shader hole and texture swap either
//! gives the mesh another material or edits its material. [`follow`] runs in `PostUpdate` after
//! those producers, the mesh terrain blend's conversion and the asset events, and before the
//! visibility check. That place matters [documented: engine/vendor bevy_render camera.rs
//! `extract_cameras`, `iter_to_dequeue`; bevy_pbr material.rs
//! `check_entities_needing_specialization`]: the renderer's draw bins are kept from frame to
//! frame, and an entity leaves them when it left `VisibleEntities` or is listed by that
//! check, both made in `PostUpdate`; despawned in `Last` it would be drawn once more. Despawned
//! here it is gone from the frame that shows the edit. The core's own material can only
//! follow a frame late (its write is an asset event of the next frame), so:
//! - an edit of what sets the alpha outline ([`Outline`]: texture transform, base texture,
//!   alpha, cut, holes) takes the core away at once, and the material has none again for as
//!   long as it is in the store (it is animated; while the light is off nothing is watched:
//!   the material itself is marked, so one put in anew under its id is told apart when the light
//!   comes back);
//! - any other edit (emissive, gloss) is written to the core's material, a frame late, which the
//!   blended draw over it covers.
//!
//! Left out whole: skinned and morphing meshes, a mesh under a `MaterialOverride`, everything under an actor's rig or a body fading at a door. Under the
//! mesh terrain blend a shape is drawn by a clone with the landscape's colours near the ground and
//! the same alpha: its core is the plain material's, under the clone's draw, and it has none while
//! the clone is another source's ([`drawn_source`]).
//!
//! **Cost.** Off: two run conditions. On: a pass over the placed meshes' change ticks, one lookup
//! and comparison a core ([`place`]), the frame's material events and removals; the textures' alpha is read once per file on a
//! background task, only for materials that pass the other rules.

use crate::door_fade::DoorFade;
use crate::limbs::ActorRig;
use crate::medium_air_demand::{AirDraws, AirOpen};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers, VisibilityRange, VisibilitySystems};
use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::ecs::system::SystemParam;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::Affine2;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::PreviousGlobalTransform;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task};
use bevy::transform::TransformSystems;
use nvf_assets::morph::PlayingMorph;
use nvf_assets::nif_material::MaterialOverride;
use nvf_assets::nif_spawn::{solid_core, SOLID_CORE_CUT};
use nvf_assets::texture::SolidTexels;
use nvf_assets::nif_surface_blend::{BlendShape, NifBlendMaterial};
use nvf_assets::{GameData, NifMaterial, TextureCache};
use nvf_play::RefVisual;

/// The debug aid (not in release builds, dev_aids.rs), read once at the start: `NVF_PLAY_DEBUG_SOLID_CORES=off` draws no cores,
/// `=ref:<FormID hex>` logs one reference's cores once.
pub const AID: &str = "NVF_PLAY_DEBUG_SOLID_CORES";

/// What the aid asks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Aid {
    #[default]
    None,
    Off,
    Reference(u32),
}

impl Aid {
    fn parse(v: Option<&str>) -> Aid {
        let v = v.unwrap_or_default().trim().to_ascii_lowercase();
        if v == "off" {
            return Aid::Off;
        }
        let id = v.strip_prefix("ref:").map(|id| id.trim().trim_start_matches("0x")).and_then(|id| u32::from_str_radix(id, 16).ok());
        id.map_or(Aid::None, Aid::Reference)
    }
}

/// On a core's entity: the shape it is the core of, and whether it was made with a visibility
/// range (its pipelines are chosen with or without the dither when it is made; fade.rs).
#[derive(Component)]
pub struct SolidCore {
    pub shape: Entity,
    ranged: bool,
}

/// A watched mesh: the material it draws with and its core.
struct Watched {
    source: AssetId<NifMaterial>,
    core: Option<Entity>,
}

/// What sets a material's alpha outline on its mesh: a core drawn with an older one than
/// its shape's would be solid where the shape no longer is. The alpha mode is not in it: a
/// material that is no plain blend has no core at all, and one that is again gets a new one.
#[derive(Clone, PartialEq)]
struct Outline {
    uv: Affine2,
    texture: Option<AssetId<Image>>,
    alpha: f32,
    cut: f32,
    holes: Vec4,
}

impl Outline {
    fn of(m: &NifMaterial) -> Outline {
        Outline { holes: m.extension.holes, ..Outline::of_parts(&m.base, m.extension.alpha_cut) }
    }

    fn of_parts(base: &StandardMaterial, cut: f32) -> Outline {
        Outline {
            uv: base.uv_transform,
            texture: base.base_color_texture.as_ref().map(Handle::id),
            alpha: base.base_color.alpha(),
            cut,
            holes: nvf_assets::nif_material::NO_HOLES,
        }
    }
}

/// A watched material: the meshes that wear it and the material of their cores.
#[derive(Default)]
struct Source {
    users: EntityHashSet,
    core: Option<Handle<NifMaterial>>,
    /// As it was last seen; `None` until it is in the store.
    outline: Option<Outline>,
}

/// The cores drawn and what they follow.
#[derive(Resource, Default)]
pub struct SolidCores {
    aid: Aid,
    said_off: bool,
    /// Cores are followed (the light was wanted at the last run).
    on: bool,
    watched: EntityHashMap<Watched>,
    sources: HashMap<AssetId<NifMaterial>, Source>,
    /// Materials whose outline changed while watched: no core as long as they are in the
    /// store, whatever wears them and however often the light is switched.
    animated: HashSet<AssetId<NifMaterial>>,
    /// Whether a texture has a solid texel, by the path the material names.
    verdicts: HashMap<String, SolidTexels>,
    /// Textures being read for it.
    asked: HashMap<String, Task<SolidTexels>>,
    /// Watched meshes with a core.
    count: usize,
    log: CountLog,
    /// The aid's reference was logged.
    said_reference: bool,
    said_terrain: bool,
}

/// Seconds the count rests before it is logged [design].
const REST_SECS: f32 = 1.0;

/// Seconds the count rests before the aid's reference is logged with nothing of it in view
/// [design: the view may still be turning to it, or the cells loading].
const REFERENCE_SECS: f32 = 20.0;

/// The count line: said once the count has rested, and again when it is notably another.
#[derive(Default)]
struct CountLog {
    said: Option<usize>,
    last: usize,
    since: f32,
    /// Seconds the count has rested.
    rest: f32,
}

impl CountLog {
    /// Whether to say `count` at `now` (`busy`: textures are still being read).
    fn say(&mut self, count: usize, now: f32, busy: bool) -> bool {
        if count != self.last {
            self.last = count;
            self.since = now;
        }
        self.rest = now - self.since;
        if busy || self.rest < REST_SECS || !notable(self.said, count) {
            return false;
        }
        self.said = Some(count);
        true
    }
}

/// Whether a count is worth a line after `said`: the first above 0, then one that comes from
/// or goes to 0 or differs by a quarter (at least 16) [design].
fn notable(said: Option<usize>, count: usize) -> bool {
    match said {
        None => count > 0,
        Some(s) => s != count && (s == 0 || count == 0 || s.abs_diff(count) >= (s / 4).max(16)),
    }
}

/// Whether to say the aid's reference now: something of it is in view, or nothing for long.
fn say_reference(rested: f32, busy: bool, in_view: usize) -> bool {
    !busy && (rested >= REFERENCE_SECS || (rested >= REST_SECS && in_view > 0))
}

/// The cores' systems of a frame ([`follow`], [`place`]), after the light's gate.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CoreSystems;

pub struct SolidCoresPlugin;

impl Plugin for SolidCoresPlugin {
    fn build(&self, app: &mut App) {
        let aid = Aid::parse(crate::dev_aids::var(AID).as_deref());
        app.insert_resource(SolidCores { aid, ..default() }).add_systems(
            PostUpdate,
            (follow, place)
                .chain()
                .in_set(CoreSystems)
                .after(crate::nif_mirror::MirrorSystems)
                .after(nvf_assets::nif_material::BlendedCutoutSystems)
                .after(crate::mesh_terrain_blend::BlendConversion)
                .after(bevy::asset::AssetEventSystems)
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate)
                .before(VisibilitySystems::CheckVisibility)
                .before(bevy::pbr::check_entities_needing_specialization::<NifMaterial>)
                .run_if(needed),
        );
        // After the visibility check, whose answer it reports.
        if crate::dev_aids::ON {
            app.add_systems(Last, report_reference.run_if(reference_asked));
        }
    }
}

/// The aid names a reference that is not logged yet.
fn reference_asked(state: Res<SolidCores>) -> bool {
    state.on && !(state.said_reference && state.said_terrain) && matches!(state.aid, Aid::Reference(_))
}

/// The aid's reference, once the count has rested: its cores, and how many of them the frame's
/// visibility check passed. Whether their depth is in the picture is not told by this.
fn report_reference(mut state: ResMut<SolidCores>, whose: Query<Whose>, cores: Query<(&ViewVisibility, Has<NoFrustumCulling>), With<SolidCore>>, clones: Query<(), With<MeshMaterial3d<NifBlendMaterial>>>) {
    let Aid::Reference(id) = state.aid else { return };
    if state.log.rest < REST_SECS {
        return;
    }
    let of = |e: Entity| whose.get(e).is_ok_and(|(visual, staged)| visual.map(|v| v.0).or(staged.map(|s| s.r)) == Some(id));
    let drawn: Vec<(Entity, Entity)> = state.watched.iter().filter(|(e, _)| of(**e)).filter_map(|(&e, w)| Some((e, w.core?))).collect();
    // Under a loading screen every mesh is drawn unculled (pipeline_prewarm.rs): not in view.
    let in_view = drawn.iter().filter(|&&(_, c)| cores.get(c).is_ok_and(|(seen, unculled)| seen.get() && !unculled)).count();
    if !state.said_reference && say_reference(state.log.rest, !state.asked.is_empty(), in_view) {
        state.said_reference = true;
        info!("solid cores: reference {id:08X}: {} cores ({in_view} of them in view)", drawn.len());
    }
    // Apart, for the conversion may come later: of the shapes the mesh terrain blend draws.
    let shown = |&&(e, c): &&(Entity, Entity)| clones.contains(e) && cores.get(c).is_ok_and(|(seen, unculled)| seen.get() && !unculled);
    let terrain = drawn.iter().filter(shown).count();
    if terrain > 0 && !state.said_terrain {
        state.said_terrain = true;
        info!("solid cores: reference {id:08X}: {terrain} cores in view under a terrain blend clone");
    }
}

/// Cores are wanted, or some are still followed and have to go.
fn needed(wanted: Option<Res<AirDraws>>, state: Res<SolidCores>) -> bool {
    state.on || wanted.is_some_and(|w| w.0)
}

/// A mesh of a placed reference.
type Placed = (Or<(With<RefVisual>, With<crate::destroyed::StagedShape>)>, With<Mesh3d>, Without<SolidCore>);
/// Something a core copies or asks of its shape was added or changed (removals: [`Lost`]).
type Moved = Or<(
    Added<RefVisual>,
    Added<crate::destroyed::StagedShape>,
    Changed<Mesh3d>,
    Changed<MeshMaterial3d<NifMaterial>>,
    Changed<MeshMaterial3d<NifBlendMaterial>>,
    Changed<BlendShape>,
    Changed<VisibilityRange>,
    Changed<RenderLayers>,
    Added<NotShadowReceiver>,
    Added<SkinnedMesh>,
    Added<PlayingMorph>,
    Added<MaterialOverride>,
    Changed<ChildOf>,
)>;

/// The meshes that lost something a core copies or asks of its shape (despawned ones too).
#[derive(SystemParam)]
struct Lost<'w, 's> {
    mesh: RemovedComponents<'w, 's, Mesh3d>,
    visual: RemovedComponents<'w, 's, RefVisual>,
    staged: RemovedComponents<'w, 's, crate::destroyed::StagedShape>,
    plain: RemovedComponents<'w, 's, MeshMaterial3d<NifMaterial>>,
    blend: RemovedComponents<'w, 's, MeshMaterial3d<NifBlendMaterial>>,
    terrain: RemovedComponents<'w, 's, BlendShape>,
    range: RemovedComponents<'w, 's, VisibilityRange>,
    layers: RemovedComponents<'w, 's, RenderLayers>,
    receiver: RemovedComponents<'w, 's, NotShadowReceiver>,
    skin: RemovedComponents<'w, 's, SkinnedMesh>,
    morph: RemovedComponents<'w, 's, PlayingMorph>,
    cloak: RemovedComponents<'w, 's, MaterialOverride>,
}

impl Lost<'_, '_> {
    /// Every such entity since the last call, into `out`.
    fn drain(&mut self, out: &mut EntityHashSet) {
        out.extend(self.mesh.read());
        out.extend(self.visual.read());
        out.extend(self.staged.read());
        out.extend(self.plain.read());
        out.extend(self.blend.read());
        out.extend(self.terrain.read());
        out.extend(self.range.read());
        out.extend(self.layers.read());
        out.extend(self.receiver.read());
        out.extend(self.skin.read());
        out.extend(self.morph.read());
        out.extend(self.cloak.read());
    }
}

type Shape = (
    &'static Mesh3d,
    Option<&'static MeshMaterial3d<NifMaterial>>,
    (Option<&'static VisibilityRange>, Option<&'static RenderLayers>, Has<NotShadowReceiver>),
    Option<&'static Aabb>,
    (&'static GlobalTransform, Option<&'static PreviousGlobalTransform>, &'static InheritedVisibility),
    (Option<&'static MeshMaterial3d<NifBlendMaterial>>, Option<&'static BlendShape>),
    (Has<SkinnedMesh>, Has<PlayingMorph>, Has<MaterialOverride>),
);
type Core = (
    &'static SolidCore,
    &'static Mesh3d,
    &'static MeshMaterial3d<NifMaterial>,
    (Option<&'static VisibilityRange>, Option<&'static RenderLayers>, Has<NotShadowReceiver>),
);
/// The plain material a shape's draw follows: its own, or under the mesh terrain blend the one
/// its clone was made from (mesh_terrain_blend.rs: the clone draws in its place, with the same
/// alpha). `None` while both draw or the clone is not (or not yet) the source's, its alpha
/// outline another's or not drawn blended (a cut-out clone would lie at the core's depth): the
/// blend keeps its old clone until the replacement is ready.
fn drawn_source(
    plain: Option<&MeshMaterial3d<NifMaterial>>,
    clone: Option<&MeshMaterial3d<NifBlendMaterial>>,
    terrain: Option<&BlendShape>,
    materials: &Assets<NifMaterial>,
    blends: Option<&Assets<NifBlendMaterial>>,
) -> Option<AssetId<NifMaterial>> {
    match (plain, clone) {
        (Some(m), None) => Some(m.0.id()),
        (None, Some(clone)) => {
            let source = terrain?.original.id();
            let (m, c) = (materials.get(source)?, blends?.get(&clone.0)?);
            (matches!(c.base.alpha_mode, AlphaMode::Blend) && Outline::of(m) == Outline { holes: m.extension.holes, ..Outline::of_parts(&c.base, c.extension.alpha_cut) }).then_some(source)
        }
        _ => None,
    }
}
/// The reference a mesh draws.
type Whose = (Option<&'static RefVisual>, Option<&'static crate::destroyed::StagedShape>);

/// Keep every placed shape's core as the module notes say.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn follow(
    mut commands: Commands,
    time: Res<Time<Real>>,
    wanted: Option<Res<AirDraws>>,
    mut state: ResMut<SolidCores>,
    (mut materials, blends): (ResMut<Assets<NifMaterial>>, Option<Res<Assets<NifBlendMaterial>>>),
    (textures, data): (Option<Res<TextureCache>>, Option<Res<GameData>>),
    mut events: MessageReader<AssetEvent<NifMaterial>>,
    mut lost: Lost,
    (all, moved): (Query<Entity, Placed>, Query<Entity, (Placed, Moved)>),
    shapes: Query<Shape, Placed>,
    cores: Query<Core>,
    core_entities: Query<Entity, With<SolidCore>>,
    (parents, rigs): (Query<&ChildOf>, Query<(), Or<(With<ActorRig>, With<DoorFade>)>>),
    mut look: Local<EntityHashSet>,
) {
    let st = &mut *state;
    let want = wanted.is_some_and(|w| w.0);
    look.clear();
    lost.drain(&mut look);
    if !want || (crate::dev_aids::ON && st.aid == Aid::Off) {
        if want && !st.said_off {
            st.said_off = true;
            info!("solid cores: off (debug aid {AID})");
        }
        st.forget_removed(events.read());
        if st.on {
            let n = core_entities.iter().count();
            for e in &core_entities {
                commands.entity(e).try_despawn();
            }
            if st.log.said.is_some() || n > 0 {
                info!("solid cores: removed ({n} draws; the light in the air is not drawn)");
            }
            // Known as such when the light comes back: no edit, so no event of one.
            for &id in &st.animated {
                if let Some(source) = materials.get_mut_untracked(id).and_then(|m| m.extension.solid_core.as_mut()) {
                    source.animated = true;
                }
            }
            st.forget();
        }
        return;
    }

    // The meshes to look at.
    if st.on {
        look.extend(moved.iter());
    } else {
        st.on = true;
        st.log.since = time.elapsed_secs();
        st.forget_removed(events.read());
        // Those gone since, or put in anew under their id (without the mark), are forgotten.
        st.animated.retain(|id| materials.get(*id).and_then(|m| m.extension.solid_core.as_ref()).is_some_and(|s| s.animated));
        look.extend(all.iter());
    }
    // The materials and the meshes whose cores have to be settled.
    let mut changed_sources: HashSet<AssetId<NifMaterial>> = HashSet::default();
    let mut settle = EntityHashSet::default();
    for &e in look.iter() {
        let source = shapes.get(e).ok().and_then(|(_, material, _, _, _, (clone, terrain), (skinned, morphs, cloaked))| {
            drawn_source(material, clone, terrain, &materials, blends.as_deref())
                .filter(|_| !skinned && !morphs && !cloaked)
                // A material a worker has not put in yet is watched until it is there.
                .filter(|&id| materials.get(id).is_none_or(|m| m.extension.solid_core.is_some()))
                .filter(|_| !parents.iter_ancestors(e).any(|a| rigs.contains(a)))
        });
        match source {
            None => st.leave(e, &mut commands),
            Some(id) => {
                if st.watch(e, id) {
                    changed_sources.insert(id);
                }
                settle.insert(e);
            }
        }
    }
    for event in events.read() {
        match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id } => {
                if st.sources.contains_key(id) {
                    changed_sources.insert(*id);
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                st.animated.remove(id);
                // Worn still: its core goes with it, and what comes under its id is another
                // material, whose outline is compared with nothing.
                if let Some(source) = st.sources.get_mut(id) {
                    source.outline = None;
                    changed_sources.insert(*id);
                }
            }
        }
    }

    // The textures read since (nothing to look at while none is asked).
    if !st.asked.is_empty() {
        let mut read: HashSet<String> = HashSet::default();
        for (path, task) in st.asked.iter_mut() {
            if let Some(answer) = check_ready(task) {
                st.verdicts.insert(path.clone(), answer);
                read.insert(path.clone());
            }
        }
        if !read.is_empty() {
            st.asked.retain(|path, _| !read.contains(path));
            changed_sources.extend(st.sources.keys().copied().filter(|&id| {
                materials.get(id).and_then(|m| m.extension.solid_core.as_ref()).is_some_and(|s| read.contains(&s.texture))
            }));
        }
    }

    // Each changed material's core.
    for id in changed_sources {
        let Some(source) = st.sources.get_mut(&id) else { continue };
        let (candidate, made) = match materials.get(id) {
            // Not in the store yet.
            None => (true, None),
            Some(m) => match &m.extension.solid_core {
                None => (false, None),
                Some(s) => {
                    let outline = Outline::of(m);
                    if source.outline.as_ref().is_some_and(|was| *was != outline) {
                        st.animated.insert(id);
                    }
                    source.outline = Some(outline);
                    // How the texture's alpha is, asked only for a material that can have a core.
                    let able = !st.animated.contains(&id) && solid_core(m, SOLID_CORE_CUT).is_some();
                    let solid = able
                        .then(|| match st.verdicts.get(&s.texture) {
                            Some(answer) => *answer,
                            None => {
                                if let (Some(textures), Some(data), false) = (&textures, &data, st.asked.contains_key(&s.texture)) {
                                    let (workers, data, path) = (textures.worker_textures(), (**data).clone(), s.texture.clone());
                                    let task = AsyncComputeTaskPool::get().spawn(async move { workers.solid_texels(&path, &data) });
                                    st.asked.insert(s.texture.clone(), task);
                                }
                                SolidTexels::None
                            }
                        })
                        .and_then(SolidTexels::cut);
                    (true, solid.and_then(|cut| solid_core(m, cut)))
                }
            },
        };
        source.core = match (made, source.core.take()) {
            (Some(core), Some(handle)) => {
                if let Err(e) = materials.insert(handle.id(), core) {
                    debug!("solid cores: {e}");
                }
                Some(handle)
            }
            (Some(core), None) => Some(materials.add(core)),
            (None, _) => None,
        };
        settle.extend(source.users.iter().copied());
        if !candidate {
            // Watched before its material was in the store; it can have no core.
            for e in source.users.iter().copied().collect::<Vec<_>>() {
                st.leave(e, &mut commands);
            }
        }
    }

    // Each shape's core as its material's says.
    for e in settle {
        let Some(w) = st.watched.get_mut(&e) else { continue };
        let Ok((mesh, plain, (range, layers, no_receive), aabb, (at, before, inherited), (clone, terrain), _)) = shapes.get(e) else { continue };
        // Asked again: a source edited in place may be drawn by an older clone for now.
        let follows = drawn_source(plain, clone, terrain, &materials, blends.as_deref()) == Some(w.source);
        let Some(material) = st.sources.get(&w.source).and_then(|s| s.core.clone()).filter(|_| follows) else {
            if let Some(core) = w.core.take() {
                commands.entity(core).try_despawn();
                st.count -= 1;
            }
            continue;
        };
        if let Some(core) = w.core.take() {
            if let Ok((made, core_mesh, core_material, (core_range, core_layers, core_no_receive))) = cores.get(core) {
                // Made anew with another mesh, another material or the dither's first (or last)
                // range.
                if core_mesh == mesh && core_material.0 == material && made.ranged == range.is_some() {
                    let mut c = commands.entity(core);
                    if let Some(range) = range.filter(|&r| core_range != Some(r)) {
                        c.try_insert(range.clone());
                    }
                    if core_layers != layers {
                        match layers {
                            Some(layers) => c.try_insert(layers.clone()),
                            None => c.try_remove::<RenderLayers>(),
                        };
                    }
                    match (no_receive, core_no_receive) {
                        (true, false) => {
                            c.try_insert(NotShadowReceiver);
                        }
                        (false, true) => {
                            c.try_remove::<NotShadowReceiver>();
                        }
                        _ => {}
                    }
                    w.core = Some(core);
                    continue;
                }
            }
            commands.entity(core).try_despawn();
            st.count -= 1;
        }
        let mut core = commands.spawn((
            SolidCore { shape: e, ranged: range.is_some() },
            mesh.clone(),
            MeshMaterial3d(material),
            // Where its shape is and as it shows: this frame's propagation is over.
            *at,
            Visibility::Inherited,
            *inherited,
            NotShadowCaster,
        ));
        // Required by the mesh; without them no propagation touches the core (module notes).
        core.remove::<(Transform, Visibility)>();
        if let Some(range) = range {
            core.insert(range.clone());
        }
        if let Some(layers) = layers {
            core.insert(layers.clone());
        }
        if let Some(aabb) = aabb {
            core.insert(*aabb);
        }
        // Its shape's motion in its first frame too (PreUpdate, which keeps it from then on, is
        // over).
        if let Some(before) = before {
            core.insert(before.clone());
        }
        if no_receive {
            core.insert(NotShadowReceiver);
        }
        w.core = Some(core.id());
        st.count += 1;
    }

    let (count, busy) = (st.count, !st.asked.is_empty());
    if st.log.say(count, time.elapsed_secs(), busy) {
        let made = st.sources.values().filter(|s| s.core.is_some()).count();
        info!("solid cores: {count} (draws of {made} materials, for the light in the air)");
    }
}

/// Each core where its shape is and shown as it is, bit for bit (module notes), but not in a frame
/// whose gate is closed ([`AirOpen`]: the light is not composed; the cores go once that has
/// settled); one that is not the core kept for its shape goes.
fn place(
    mut commands: Commands,
    (state, open): (Res<SolidCores>, Option<Res<AirOpen>>),
    shapes: Query<(&GlobalTransform, &InheritedVisibility), Without<SolidCore>>,
    mut cores: Query<(Entity, &SolidCore, &mut GlobalTransform, &mut InheritedVisibility)>,
) {
    let open = open.is_none_or(|o| o.0);
    for (e, core, mut at, mut inherited) in &mut cores {
        let kept = state.watched.get(&core.shape).is_some_and(|w| w.core == Some(e));
        match shapes.get(core.shape) {
            Ok((shape_at, shape_inherited)) if kept => {
                at.set_if_neq(*shape_at);
                inherited.set_if_neq(if open { *shape_inherited } else { InheritedVisibility::HIDDEN });
            }
            _ => commands.entity(e).try_despawn(),
        }
    }
}

impl SolidCores {
    /// Materials gone from the store (or put in anew) are no longer known as animated.
    fn forget_removed<'a>(&mut self, events: impl Iterator<Item = &'a AssetEvent<NifMaterial>>) {
        for event in events {
            if let AssetEvent::Removed { id } | AssetEvent::Unused { id } = event {
                self.animated.remove(id);
            }
        }
    }

    /// Nothing is followed any more; what was read of the textures and which materials are
    /// animated stay known.
    fn forget(&mut self) {
        self.on = false;
        self.watched.clear();
        self.sources.clear();
        self.asked.clear();
        self.count = 0;
        self.log = CountLog::default();
    }

    /// Stop watching `e`; its core goes.
    fn leave(&mut self, e: Entity, commands: &mut Commands) {
        let Some(w) = self.watched.remove(&e) else { return };
        self.drop_user(w.source, e);
        if let Some(core) = w.core {
            commands.entity(core).try_despawn();
            self.count -= 1;
        }
    }

    /// Watch `e` with the material `id`: whether the material is a newly watched one. A core
    /// it has stays until it is settled.
    fn watch(&mut self, e: Entity, id: AssetId<NifMaterial>) -> bool {
        match self.watched.get_mut(&e) {
            Some(w) if w.source == id => return false,
            Some(w) => {
                let old = std::mem::replace(&mut w.source, id);
                self.drop_user(old, e);
            }
            None => {
                self.watched.insert(e, Watched { source: id, core: None });
            }
        }
        let new = !self.sources.contains_key(&id);
        self.sources.entry(id).or_default().users.insert(e);
        new
    }

    /// `e` no longer wears `id`: the source and its core material go with their last user.
    fn drop_user(&mut self, id: AssetId<NifMaterial>, e: Entity) {
        if let Some(s) = self.sources.get_mut(&id) {
            s.users.remove(&e);
            if s.users.is_empty() {
                self.sources.remove(&id);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fade;
    use nvf_assets::nif_material::{nif_material, SolidCoreSource};

    const TEXTURE: &str = "tank.dds";

    /// A blended material whose file rules allow a core.
    pub(crate) fn tank() -> NifMaterial {
        let image = Handle::<Image>::default();
        let mut m = nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, base_color_texture: Some(image.clone()), ..default() });
        m.extension.solid_core = Some(SolidCoreSource { texture: TEXTURE.into(), image: image.id(), animated: false });
        m
    }

    /// An app that follows cores with the light wanted; `solid`: whether the tank's texture has
    /// a solid texel, unless it is not known.
    pub(crate) fn app(solid: Option<bool>) -> App {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<NifMaterial>().init_asset::<Mesh>().init_asset::<Image>().init_resource::<Time<Real>>();
        app.add_plugins(SolidCoresPlugin).insert_resource(SolidCores::default()).insert_resource(AirDraws(true));
        if let Some(solid) = solid {
            app.world_mut().resource_mut::<SolidCores>().verdicts.insert(TEXTURE.into(), if solid { SolidTexels::Mostly } else { SolidTexels::None });
        }
        app
    }

    pub(crate) fn add(app: &mut App, m: NifMaterial) -> Handle<NifMaterial> {
        app.world_mut().resource_mut::<Assets<NifMaterial>>().add(m)
    }

    fn mesh(app: &mut App) -> Handle<Mesh> {
        app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default()))
    }

    pub(crate) fn place(app: &mut App, m: &Handle<NifMaterial>) -> Entity {
        let mesh = mesh(app);
        // Visibility: the game's camera plugin requires it of a Mesh3d; there is none here.
        app.world_mut().spawn((RefVisual(0x106B5E), Mesh3d(mesh), MeshMaterial3d(m.clone()), Transform::from_xyz(1.0, 2.0, 3.0), Visibility::Inherited)).id()
    }

    pub(crate) fn cores(app: &mut App) -> Vec<Entity> {
        app.world_mut().query_filtered::<Entity, With<SolidCore>>().iter(app.world()).collect()
    }

    /// The number of cores, checked against the count kept along the way.
    fn count(app: &mut App) -> usize {
        let n = cores(app).len();
        assert_eq!(app.world().resource::<SolidCores>().count, n, "the count is kept");
        n
    }

    fn set(app: &mut App, e: Entity, m: &Handle<NifMaterial>) {
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(m.clone()));
        app.update();
    }

    fn edit(app: &mut App, m: &Handle<NifMaterial>, how: impl FnOnce(&mut NifMaterial)) {
        how(&mut app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(m).unwrap());
        app.update();
    }

    #[test]
    fn a_placed_blend_gets_one_core_that_casts_nothing_and_ends_with_it() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        let (a, b) = (place(&mut app, &m), place(&mut app, &m));
        app.world_mut().entity_mut(b).insert(RenderLayers::layer(3));
        app.update();
        let made = cores(&mut app);
        assert_eq!((made.len(), count(&mut app)), (2, 2));
        let w = app.world();
        let core = made.iter().copied().find(|&c| w.get::<SolidCore>(c).unwrap().shape == a).unwrap();
        assert!(w.get::<NotShadowCaster>(core).is_some() && w.get::<RefVisual>(core).is_none());
        assert!(w.get::<ChildOf>(core).is_none() && w.get::<Transform>(core).is_none() && w.get::<Visibility>(core).is_none());
        assert!(w.get::<Children>(a).is_none());
        assert_eq!(w.get::<GlobalTransform>(core), w.get::<GlobalTransform>(a));
        assert_eq!(w.get::<Mesh3d>(core).unwrap().0, w.get::<Mesh3d>(a).unwrap().0);
        let other = made.iter().copied().find(|&c| c != core).unwrap();
        assert_eq!(w.get::<RenderLayers>(other), Some(&RenderLayers::layer(3)));
        // One core material for both, a hard cut-out; the shapes keep theirs.
        let h = w.get::<MeshMaterial3d<NifMaterial>>(core).unwrap().0.clone();
        assert_eq!(h, w.get::<MeshMaterial3d<NifMaterial>>(other).unwrap().0);
        let drawn = w.resource::<Assets<NifMaterial>>().get(&h).unwrap();
        assert_eq!(drawn.base.alpha_mode, AlphaMode::Mask(SOLID_CORE_CUT));
        assert_eq!(w.get::<MeshMaterial3d<NifMaterial>>(a).unwrap().0, m);
        // Nothing is made again.
        app.update();
        let mut again = cores(&mut app);
        again.sort();
        let mut first = made.clone();
        first.sort();
        assert_eq!(again, first);
        // A shape's end is its core's; the material goes with the last.
        app.world_mut().despawn(a);
        app.update();
        assert_eq!((cores(&mut app), count(&mut app)), (vec![other], 1));
        app.world_mut().despawn(b);
        app.update();
        assert_eq!(count(&mut app), 0);
        let st = app.world().resource::<SolidCores>();
        assert!(st.watched.is_empty() && st.sources.is_empty());
    }

    #[test]
    fn nothing_is_made_while_the_light_is_not_wanted_and_all_goes_with_the_row() {
        let mut app = app(Some(true));
        app.insert_resource(AirDraws(false));
        let m = add(&mut app, tank());
        place(&mut app, &m);
        app.update();
        app.update();
        assert_eq!(count(&mut app), 0);
        assert!(app.world().resource::<SolidCores>().watched.is_empty());
        assert_eq!(app.world().resource::<Assets<NifMaterial>>().len(), 1);
        // On in play: what was drawn before gets its core; off again: none, and back.
        for _ in 0..2 {
            app.insert_resource(AirDraws(true));
            app.update();
            assert_eq!(count(&mut app), 1);
            app.insert_resource(AirDraws(false));
            app.update();
            assert_eq!(count(&mut app), 0);
            assert!(app.world().resource::<SolidCores>().sources.is_empty());
        }
        // The aid keeps them off.
        app.world_mut().resource_mut::<SolidCores>().aid = Aid::Off;
        app.insert_resource(AirDraws(true));
        app.update();
        assert_eq!(count(&mut app), 0);
    }

    #[test]
    fn the_aid_switches_the_cores_off_or_names_a_reference() {
        assert_eq!(Aid::parse(Some(" Off ")), Aid::Off);
        for on in [None, Some(""), Some("on"), Some("ref:"), Some("ref:tank"), Some("00106B5E")] {
            assert_eq!(Aid::parse(on), Aid::None, "{on:?}");
        }
        for id in ["ref:00106B5E", "ref:106b5e", " REF:0x00106B5E "] {
            assert_eq!(Aid::parse(Some(id)), Aid::Reference(0x106B5E), "{id:?}");
        }
        assert!(!say_reference(0.5, false, 1), "not at rest");
        assert!(!say_reference(5.0, false, 0), "the view may still turn");
        assert!(!say_reference(30.0, true, 1), "textures being read");
        assert!(say_reference(1.0, false, 1) && say_reference(REFERENCE_SECS, false, 0));
    }

    /// A fade's copy, FSR's alpha test, a swapped texture, an opaque material: the core goes in
    /// that frame, and is back when the shape wears its own again.
    #[test]
    fn a_core_goes_with_another_material_and_comes_back() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        let e = place(&mut app, &m);
        app.update();
        assert_eq!(count(&mut app), 1);
        let mut faded = tank();
        faded.base.base_color.set_alpha(0.5);
        let mut tested = tank();
        tested.base.alpha_mode = AlphaMode::Mask(0.5);
        let mut swapped = tank();
        swapped.base.base_color_texture = Some(bevy::asset::uuid_handle!("6d0f3f0e-6f5b-4a1c-9a64-2f8f5c7d1e91"));
        let plain = nif_material(StandardMaterial::default());
        for other in [faded, tested, swapped, plain] {
            let h = add(&mut app, other);
            set(&mut app, e, &h);
            assert_eq!(count(&mut app), 0);
            set(&mut app, e, &m);
            assert_eq!(count(&mut app), 1);
        }
        // A cloak's override, put on and taken off with no other change.
        app.world_mut().entity_mut(e).insert(MaterialOverride { source: m.clone(), copy: m.clone() });
        app.update();
        assert_eq!(count(&mut app), 0);
        app.world_mut().entity_mut(e).remove::<MaterialOverride>();
        app.update();
        assert_eq!(count(&mut app), 1);
    }

    /// The same material edited in place. What moves the alpha outline takes the core away in
    /// the frame the edit is extracted (the core's own material would follow a frame late), for
    /// as long as the material is in the store, whoever wears it and however the light is
    /// switched; FSR's alpha-mode switch only while it lasts; the rest is followed.
    #[test]
    fn an_edit_of_the_outline_ends_the_core_and_any_other_is_followed() {
        type Edit = fn(&mut NifMaterial);
        let outline: [(&str, Edit, Edit); 4] = [
            ("the texture transform", |m| m.base.uv_transform = Affine2::from_translation(Vec2::new(0.25, 0.0)), |m| m.base.uv_transform = Affine2::IDENTITY),
            ("alpha", |m| m.base.base_color.set_alpha(0.9), |m| m.base.base_color.set_alpha(1.0)),
            ("an alpha cut", |m| m.extension.alpha_cut = 0.5, |m| m.extension.alpha_cut = 0.0),
            (
                "the base texture",
                |m| m.base.base_color_texture = Some(bevy::asset::uuid_handle!("6d0f3f0e-6f5b-4a1c-9a64-2f8f5c7d1e92")),
                |m| m.base.base_color_texture = Some(Handle::default()),
            ),
        ];
        for (what, change, back) in outline {
            let mut app = app(Some(true));
            let m = add(&mut app, tank());
            let e = place(&mut app, &m);
            app.update();
            assert_eq!(count(&mut app), 1);
            edit(&mut app, &m, change);
            assert_eq!(count(&mut app), 0, "{what}: gone in the edit's own frame");
            edit(&mut app, &m, back);
            app.update();
            assert_eq!(count(&mut app), 0, "{what}: animated, none again");
            // Another wearer gets none either, nor the next after the last has gone.
            let other = place(&mut app, &m);
            app.update();
            assert_eq!(count(&mut app), 0, "{what}");
            app.world_mut().despawn(e);
            app.world_mut().despawn(other);
            app.update();
            place(&mut app, &m);
            app.update();
            assert_eq!(count(&mut app), 0, "{what}: its last wearer gone");
            // The light off and on.
            for on in [false, true] {
                app.insert_resource(AirDraws(on));
                app.update();
            }
            assert_eq!(count(&mut app), 0, "{what}: the light switched");
            // Forgotten with the asset, followed or not.
            assert_eq!(app.world().resource::<SolidCores>().animated.len(), 1);
            if what == "alpha" {
                app.insert_resource(AirDraws(false));
                app.update();
            }
            app.world_mut().resource_mut::<Assets<NifMaterial>>().remove(&m);
            app.update();
            app.insert_resource(AirDraws(true));
            app.update();
            assert!(app.world().resource::<SolidCores>().animated.is_empty(), "{what}");
        }
        // With the light off nothing runs, whatever is known as animated.
        {
            use bevy::ecs::system::RunSystemOnce;
            let mut app = app(Some(true));
            let m = add(&mut app, tank());
            place(&mut app, &m);
            app.update();
            edit(&mut app, &m, |m| m.base.base_color.set_alpha(0.9));
            app.insert_resource(AirDraws(false));
            app.update();
            assert_eq!(app.world().resource::<SolidCores>().animated.len(), 1);
            assert!(!app.world_mut().run_system_once(needed).unwrap());
        }
        // A material put in again under the id while the light is off is another one.
        {
            let mut app = app(Some(true));
            let m = add(&mut app, tank());
            place(&mut app, &m);
            app.update();
            edit(&mut app, &m, |m| m.base.base_color.set_alpha(0.9));
            edit(&mut app, &m, |m| m.base.base_color.set_alpha(1.0));
            app.update();
            assert_eq!(count(&mut app), 0);
            app.insert_resource(AirDraws(false));
            app.update();
            app.world_mut().resource_mut::<Assets<NifMaterial>>().remove(&m);
            app.update();
            app.world_mut().resource_mut::<Assets<NifMaterial>>().insert(m.id(), tank()).unwrap();
            app.update();
            app.insert_resource(AirDraws(true));
            app.update();
            assert_eq!(count(&mut app), 1);
        }
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        place(&mut app, &m);
        app.update();
        edit(&mut app, &m, |m| m.base.alpha_mode = AlphaMode::Mask(0.5));
        assert_eq!(count(&mut app), 0);
        edit(&mut app, &m, |m| m.base.alpha_mode = AlphaMode::Blend);
        assert_eq!(count(&mut app), 1);
        // An emissive edit is followed, by the same core material.
        let core = cores(&mut app)[0];
        let h = app.world().get::<MeshMaterial3d<NifMaterial>>(core).unwrap().0.clone();
        edit(&mut app, &m, |m| m.base.emissive = LinearRgba::rgb(0.5, 0.1, 0.2));
        assert_eq!(cores(&mut app), vec![core]);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(core).unwrap().0, h);
        assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&h).unwrap().base.emissive, LinearRgba::rgb(0.5, 0.1, 0.2));
    }

    /// A worn material taken out of the store: its core goes with it, and comes with it again.
    #[test]
    fn a_removed_material_takes_its_visible_core() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        place(&mut app, &m);
        app.update();
        assert_eq!(count(&mut app), 1);
        app.world_mut().resource_mut::<Assets<NifMaterial>>().remove(&m);
        app.update();
        assert_eq!(count(&mut app), 0);
        app.world_mut().resource_mut::<Assets<NifMaterial>>().insert(m.id(), tank()).unwrap();
        app.update();
        assert_eq!(count(&mut app), 1);
        // Another material under the id, with another (fixed) UV transform, is not an edit.
        app.world_mut().resource_mut::<Assets<NifMaterial>>().remove(&m);
        app.update();
        assert_eq!(count(&mut app), 0);
        let mut other = tank();
        other.base.uv_transform.translation.x = 0.25;
        app.world_mut().resource_mut::<Assets<NifMaterial>>().insert(m.id(), other).unwrap();
        app.update();
        assert_eq!(count(&mut app), 1);
    }

    /// A core takes its shape's matrix of the frame before, so a core made anew under a moving
    /// shape has the shape's motion in its first frame.
    #[test]
    fn a_new_core_has_its_shapes_motion() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        let e = place(&mut app, &m);
        let before = PreviousGlobalTransform(bevy::math::Affine3A::from_translation(Vec3::new(7.0, 8.0, 9.0)));
        app.world_mut().entity_mut(e).insert(before.clone());
        app.update();
        let core = cores(&mut app)[0];
        assert!(app.world().get::<PreviousGlobalTransform>(core) == Some(&before));
        // Another material: the core is made anew.
        let h = add(&mut app, tank());
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(h));
        app.update();
        let again = cores(&mut app)[0];
        assert_ne!(again, core);
        assert!(app.world().get::<PreviousGlobalTransform>(again) == Some(&before));
    }

    /// A shape moved and turned under a scaled parent: the core's matrix is the shape's, bit for
    /// bit, in every frame's visibility check (its first, when Bevy propagates new transforms,
    /// the next and while it moves), and its visibility follows in the same frame.
    #[test]
    fn a_core_is_where_its_shape_is_bit_for_bit() {
        #[derive(Resource, Default)]
        struct Seen(Vec<(bool, bool)>);
        let mut app = app(Some(true));
        app.add_plugins(TransformPlugin).init_resource::<Seen>().add_systems(
            PostUpdate,
            (|mut seen: ResMut<Seen>, cores: Query<(&SolidCore, &GlobalTransform, &InheritedVisibility)>, shapes: Query<(&GlobalTransform, &InheritedVisibility)>| {
                for (core, at, shown) in &cores {
                    let (shape_at, shape_shown) = shapes.get(core.shape).unwrap();
                    let bits = |g: &GlobalTransform| g.to_matrix().to_cols_array().map(f32::to_bits);
                    seen.0.push((bits(at) == bits(shape_at), shown == shape_shown));
                }
            })
            .in_set(VisibilitySystems::CheckVisibility),
        );
        let m = add(&mut app, tank());
        // A parent whose uneven scale shears what is turned below it.
        let parent = app.world_mut().spawn((Transform { translation: Vec3::new(-68245.27, 590.69, 8183.14), rotation: Quat::from_rotation_z(0.7), scale: Vec3::new(1.16, 0.9, 1.3) }, Visibility::Inherited)).id();
        let e = place(&mut app, &m);
        app.world_mut().entity_mut(e).insert(ChildOf(parent));
        for frame in 0..4 {
            let turn = Transform { translation: Vec3::splat(frame as f32 * 3.3), rotation: Quat::from_rotation_y(0.3 + frame as f32), scale: Vec3::splat(0.77) };
            app.world_mut().entity_mut(e).insert((turn, if frame % 2 == 0 { InheritedVisibility::VISIBLE } else { InheritedVisibility::HIDDEN }));
            app.update();
            assert_eq!(count(&mut app), 1);
        }
        // And at rest.
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, vec![(true, true); 5]);
        let shape_at = *app.world().get::<GlobalTransform>(e).unwrap();
        assert_ne!(shape_at, GlobalTransform::IDENTITY);
        // The case for it: taken apart and put together the matrix is another.
        assert_ne!(GlobalTransform::from(shape_at.compute_transform()), shape_at);
    }

    /// A core its shape's entry does not name any more goes by itself.
    #[test]
    fn a_core_nothing_is_known_of_goes() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        place(&mut app, &m);
        app.update();
        let stray = app.world_mut().spawn((SolidCore { shape: Entity::PLACEHOLDER, ranged: false }, GlobalTransform::IDENTITY, InheritedVisibility::VISIBLE)).id();
        app.update();
        assert!(app.world().get_entity(stray).is_err());
        assert_eq!(count(&mut app), 1);
    }

    /// Another source with a core of its own: the core is made anew, never given another
    /// material in place.
    #[test]
    fn another_core_material_makes_the_core_anew() {
        let mut app = app(Some(true));
        let (a, b) = (add(&mut app, tank()), add(&mut app, tank()));
        let e = place(&mut app, &a);
        app.update();
        let core = cores(&mut app)[0];
        let first = app.world().get::<MeshMaterial3d<NifMaterial>>(core).unwrap().0.clone();
        set(&mut app, e, &b);
        let now = cores(&mut app);
        assert_eq!(count(&mut app), 1);
        assert_ne!(now[0], core);
        assert_ne!(app.world().get::<MeshMaterial3d<NifMaterial>>(now[0]).unwrap().0, first);
    }

    /// The aid's line is worked out after the visibility check: before it, every mesh's
    /// visibility is the frame's reset one.
    #[test]
    fn the_aids_reference_is_told_after_the_visibility_check() {
        #[derive(Resource)]
        struct Shown(bool);
        let mut app = app(Some(true));
        app.world_mut().resource_mut::<SolidCores>().aid = Aid::Reference(0x106B5E);
        app.insert_resource(Shown(false)).add_systems(
            PostUpdate,
            (
                (|mut seen: Query<&mut ViewVisibility>| seen.iter_mut().for_each(|mut v| *v = ViewVisibility::HIDDEN)).in_set(VisibilitySystems::VisibilityPropagate),
                (|shown: Res<Shown>, mut seen: Query<&mut ViewVisibility, With<SolidCore>>| {
                    if shown.0 {
                        seen.iter_mut().for_each(|mut v| *v = ViewVisibility::VISIBLE);
                    }
                })
                .in_set(VisibilitySystems::CheckVisibility),
            ),
        );
        let m = add(&mut app, tank());
        place(&mut app, &m);
        app.update();
        assert_eq!(count(&mut app), 1);
        // The clock stands at 0 here: at rest for two seconds.
        app.world_mut().resource_mut::<SolidCores>().log.since = -2.0;
        app.update();
        assert!(!app.world().resource::<SolidCores>().said_reference, "nothing in view");
        app.insert_resource(Shown(true));
        app.update();
        assert!(app.world().resource::<SolidCores>().said_reference);
    }

    /// The core is gone before the visibility check of the frame its shape changes in: nothing
    /// left for the renderer's retained bins to keep (module notes).
    #[test]
    fn a_core_goes_before_the_visibility_check() {
        #[derive(Resource, Default)]
        struct Seen(usize);
        let mut app = app(Some(true));
        app.init_resource::<Seen>().add_systems(
            PostUpdate,
            (|mut seen: ResMut<Seen>, cores: Query<(), With<SolidCore>>| seen.0 = cores.iter().count()).in_set(VisibilitySystems::CheckVisibility),
        );
        let m = add(&mut app, tank());
        place(&mut app, &m);
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, 1, "a new core is checked in its first frame");
        edit(&mut app, &m, |m| m.base.uv_transform = Affine2::from_translation(Vec2::X * 0.5));
        assert_eq!(app.world().resource::<Seen>().0, 0);
    }

    #[test]
    fn actors_skins_and_textures_without_a_solid_texel_have_no_core() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        let rig = app.world_mut().spawn((Transform::default(), ActorRig { reference: 1, base: 2, body: 2, skeleton: String::new(), skel: default(), nodes: Vec::new(), base_scale: 1.0 })).id();
        let leaving = app.world_mut().spawn((Transform::default(), DoorFade { since: 0.0, out: true })).id();
        for parent in [rig, leaving] {
            let e = place(&mut app, &m);
            app.world_mut().entity_mut(e).insert(ChildOf(parent));
        }
        let skinned = place(&mut app, &m);
        app.world_mut().entity_mut(skinned).insert(SkinnedMesh::default());
        // Not a placed reference's mesh.
        let h = mesh(&mut app);
        app.world_mut().spawn((Mesh3d(h), MeshMaterial3d(m.clone())));
        app.update();
        assert_eq!(count(&mut app), 0);
        // The skin taken off with no other change.
        app.world_mut().entity_mut(skinned).remove::<SkinnedMesh>();
        app.update();
        assert_eq!(count(&mut app), 1);
        app.world_mut().entity_mut(skinned).remove::<RefVisual>();
        app.update();
        assert_eq!(count(&mut app), 0, "no reference's any more");
        for solid in [Some(false), None] {
            // Without the answer (and nothing to read it with) there is no core either.
            let mut app = self::app(solid);
            let m = add(&mut app, tank());
            place(&mut app, &m);
            app.update();
            assert_eq!(count(&mut app), 0, "{solid:?}");
        }
    }

    /// The mesh terrain blend's clone draws in place of the plain material: the shape keeps its
    /// core while the clone is the source's, and has none while both draw or the clone has
    /// another alpha outline (its replacement is not ready).
    #[test]
    fn a_shape_the_terrain_blend_converted_keeps_its_core() {
        use nvf_assets::nif_surface_blend::{HeightLayout, NifSurfaceBlend, TerrainPalette};
        let clone = |m: &NifMaterial| NifBlendMaterial {
            base: m.base.clone(),
            extension: NifSurfaceBlend::from_source(&m.extension, Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Handle::default(), (Handle::default(), HeightLayout::default()), TerrainPalette::default()),
        };
        let mut app = app(Some(true));
        app.init_asset::<NifBlendMaterial>();
        let m = add(&mut app, tank());
        let e = place(&mut app, &m);
        app.update();
        assert_eq!(count(&mut app), 1);
        let core = cores(&mut app)[0];
        let drawn = app.world_mut().resource_mut::<Assets<NifBlendMaterial>>().add(clone(&tank()));
        app.world_mut().entity_mut(e).remove::<MeshMaterial3d<NifMaterial>>().insert((MeshMaterial3d(drawn.clone()), BlendShape { original: m.clone(), enabled: Some(drawn) }));
        app.update();
        assert_eq!(cores(&mut app), vec![core], "converted: the same core");
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(m.clone()));
        app.update();
        assert_eq!(count(&mut app), 0, "both on");
        app.world_mut().entity_mut(e).remove::<MeshMaterial3d<NifMaterial>>();
        app.update();
        assert_eq!(count(&mut app), 1);
        // The source is another material, the clone still the first one's.
        let mut other = tank();
        other.base.uv_transform.translation.x = 0.25;
        let source = add(&mut app, other.clone());
        app.world_mut().get_mut::<BlendShape>(e).unwrap().original = source;
        app.update();
        assert_eq!(count(&mut app), 0, "the old clone still draws");
        let ready = app.world_mut().resource_mut::<Assets<NifBlendMaterial>>().add(clone(&other));
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(ready));
        app.update();
        assert_eq!(count(&mut app), 1, "the replacement draws");
        // A clone that still draws cut out (made while the source was alpha-tested).
        let mut mask = clone(&other);
        mask.base.alpha_mode = AlphaMode::Mask(0.5);
        let mask = app.world_mut().resource_mut::<Assets<NifBlendMaterial>>().add(mask);
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(mask));
        app.update();
        assert_eq!(count(&mut app), 0, "a cut-out clone");
        // The source flips to Mask and back to Blend in place while that clone draws.
        let source = app.world().get::<BlendShape>(e).unwrap().original.clone();
        for mode in [AlphaMode::Mask(0.5), AlphaMode::Blend] {
            edit(&mut app, &source, |m| m.base.alpha_mode = mode);
            assert_eq!(count(&mut app), 0);
        }
        let again = app.world_mut().resource_mut::<Assets<NifBlendMaterial>>().add(clone(&other));
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(again));
        app.update();
        assert_eq!(count(&mut app), 1);
        app.world_mut().entity_mut(e).remove::<MeshMaterial3d<NifBlendMaterial>>().insert(MeshMaterial3d(m));
        app.update();
        assert_eq!(count(&mut app), 1, "plain again");
    }

    /// Room and plane culling (rooms.rs) hides a mesh by layers and shows it by taking them
    /// off: the same core follows both on a shape that stands still.
    #[test]
    fn layers_put_on_and_taken_off_reach_the_core() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        let e = place(&mut app, &m);
        app.update();
        let core = cores(&mut app)[0];
        assert!(app.world().get::<RenderLayers>(core).is_none());
        app.world_mut().entity_mut(e).insert((RenderLayers::layer(9), NotShadowReceiver));
        app.update();
        assert_eq!(cores(&mut app), vec![core]);
        assert_eq!(app.world().get::<RenderLayers>(core), Some(&RenderLayers::layer(9)));
        assert!(app.world().get::<NotShadowReceiver>(core).is_some());
        app.world_mut().entity_mut(e).remove::<(RenderLayers, NotShadowReceiver)>();
        app.update();
        assert_eq!(cores(&mut app), vec![core]);
        assert!(app.world().get::<RenderLayers>(core).is_none() && app.world().get::<NotShadowReceiver>(core).is_none());
    }

    /// A material a worker puts in later is watched until it is there.
    #[test]
    fn a_late_material_gets_its_core_when_it_comes() {
        let mut app = app(Some(true));
        let reserved = app.world().resource::<Assets<NifMaterial>>().reserve_handle();
        let plain = app.world().resource::<Assets<NifMaterial>>().reserve_handle();
        place(&mut app, &reserved);
        place(&mut app, &plain);
        app.update();
        assert_eq!(count(&mut app), 0);
        assert_eq!(app.world().resource::<SolidCores>().watched.len(), 2);
        let mut materials = app.world_mut().resource_mut::<Assets<NifMaterial>>();
        materials.insert(reserved.id(), tank()).unwrap();
        materials.insert(plain.id(), nif_material(StandardMaterial::default())).unwrap();
        app.update();
        assert_eq!(count(&mut app), 1);
        assert_eq!(app.world().resource::<SolidCores>().watched.len(), 1, "the other can have none");
    }

    /// A cell's fade: the core has its shape's range from the start and takes each step; a
    /// first range makes it anew (its pipelines have no dither before), and so does its end.
    #[test]
    fn a_core_takes_its_shapes_range() {
        let mut app = app(Some(true));
        let m = add(&mut app, tank());
        let e = place(&mut app, &m);
        app.update();
        let bare = cores(&mut app)[0];
        assert!(app.world().get::<VisibilityRange>(bare).is_none());
        app.world_mut().entity_mut(e).insert(fade::drawn(4));
        app.update();
        let ranged = cores(&mut app);
        assert_eq!((ranged.len(), count(&mut app)), (1, 1));
        assert_ne!(ranged[0], bare);
        assert!(app.world().get::<VisibilityRange>(ranged[0]) == Some(&fade::drawn(4)));
        app.world_mut().entity_mut(e).insert(fade::drawn(5));
        app.update();
        assert_eq!(cores(&mut app), ranged, "the same entity");
        assert!(app.world().get::<VisibilityRange>(ranged[0]) == Some(&fade::drawn(5)));
        app.world_mut().entity_mut(e).remove::<VisibilityRange>();
        app.update();
        let core = cores(&mut app)[0];
        assert!(app.world().get::<VisibilityRange>(core).is_none());
        // Another mesh (a script's, a stage's).
        let h = mesh(&mut app);
        app.world_mut().entity_mut(e).insert(Mesh3d(h.clone()));
        app.update();
        let core = cores(&mut app)[0];
        assert_eq!(app.world().get::<Mesh3d>(core).unwrap().0, h);
        assert_eq!(count(&mut app), 1);
    }

    #[test]
    fn the_count_is_said_when_it_rests_and_when_it_is_notably_another() {
        assert!(!notable(None, 0) && notable(None, 1));
        assert!(!notable(Some(40), 40) && !notable(Some(40), 50) && notable(Some(40), 56) && notable(Some(40), 0) && notable(Some(0), 1));
        assert!(!notable(Some(400), 480) && notable(Some(400), 500));
        let mut log = CountLog::default();
        assert!(!log.say(3, 10.0, false), "just changed");
        assert!(!log.say(3, 11.5, true), "textures still being read");
        assert_eq!(log.rest, 1.5);
        assert!(log.say(3, 11.5, false));
        assert!(!log.say(3, 20.0, false), "once");
        assert!(!log.say(0, 21.0, false));
        assert!(log.say(0, 22.5, false));
    }
}
