//! The drawn nodes for JIP's NIF block functions (fnv_game::nif_blocks;
//! docs/design/nif-blocks.md): each frame the scripts' node changes are applied to the
//! entities of the reference they name, and the node tree of every reference a script
//! asked about is told back. A reference's root is its actor rig (each player model
//! separately, `PlayerBody::script_rig`), told as the skeleton's root node
//! (`Scene Root` [observed: the original's head parents end there, #2132]), else the
//! topmost entity drawing it. A shape's NiMaterialProperty values (JIP's Get/SetMaterialPropertyValue)
//! are read from the reference's model file, once per model, with the scripts' changes
//! over them; a change draws the shape with its own copy of the material. The vertices of
//! the shapes a script asked for (SUP's NifBlockGetVerticles) are told back each frame.
//! A created item drawn as flat shapes (`nvf_play::dropped`, a weapon on the ground) has
//! no node entities: its tree is told from its model file (names, parents, local
//! transforms), with the scripts' set transforms over it (#2132).

use crate::limbs::ActorRig;
use bevy::prelude::*;
use bevy::camera::primitives::Aabb;
use fnv_game::nif_blocks::{ModelView, NodeInfo, SceneEdit, SceneIdentity, SceneNode, SceneTarget};
use fnv_sim::FormId;
use nvf_assets::{GameData, NifHidden, NifMaterial};
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::collections::{HashMap, HashSet};

pub struct NifMirrorPlugin;

#[path = "nif_attach_model_probe.rs"]
mod attach_model_probe;

impl Plugin for NifMirrorPlugin {
    fn build(&self, app: &mut App) {
        attach_model_probe::install(app);
        crate::camera_overlay::install(app);
        crate::nif_attachments::install(app);
        app.add_systems(
            PostUpdate,
            (crate::camera_overlay::sync, crate::nif_attachments::produce, mirror)
                .chain()
                .in_set(MirrorSystems)
                .before(TransformSystems::Propagate),
        );
    }
}

/// [design] The scripted NIF edits ([`mirror`]), for systems that must see their material
/// changes applied (mesh_terrain_blend.rs converts after them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MirrorSystems;

/// What the mirror keeps between frames: the references it logged, the held transforms,
/// the models' material values, the scripts' material values and the shapes drawn with
/// their own material since.
#[derive(Default)]
struct Memory {
    logged: std::collections::HashSet<u32>,
    // File-only models retain the explicitly legacy name-based contract.
    held: HashMap<(u32, ModelView, String), Held>,
    // Entity-backed edits live only as long as this exact model and node.
    bound_held: HashMap<(u32, ModelView, SceneIdentity), OrderedHeld>,
    // Cold writes wait by name, pin the first available model, then promote once.
    pending_held: HashMap<(u32, ModelView, String), PendingHeld>,
    edit_order: u64,
    tables: HashMap<String, HashMap<String, [f32; 9]>>,
    materials: HashMap<(u32, ModelView, String), [f32; 9]>,
    alpha_props: HashMap<(u32, ModelView, String), [f32; 2]>,
    stencil_props: HashMap<(u32, ModelView, String), [f32; 3]>,
    owned: HashMap<Entity, Handle<NifMaterial>>,
    attachment_materials: HashMap<Entity, [f32; 9]>,
    attachment_alpha: HashMap<Entity, [f32; 2]>,
    attachment_stencil: HashMap<Entity, [f32; 3]>,
    /// Each model's shapes' NiAlphaProperty and NiStencilProperty values, by shape name.
    props: HashMap<String, HashMap<String, Props>>,
    /// Each model's node tree as its file holds it, for items drawn as flat shapes.
    files: HashMap<String, Vec<FileNode>>,
    /// Incremental index of root `RefVisual` entities keyed by reference FormID, kept in
    /// sync from Bevy change detection so frames with undrawn watched refs avoid scanning
    /// every `RefVisual` entity in the world [design].
    visual_cache: VisualRootCache,
}

/// Incremental `RefVisual(r) -> root Entity` index maintained across frames via Bevy
/// change detection (`Added`/`Changed`/`RemovedComponents`) instead of scanning all
/// `RefVisual` entities every frame when undrawn references remain in `watched` [design].
#[derive(Default)]
struct VisualRootCache {
    initialized: bool,
    roots: HashMap<u32, Vec<Entity>>,
    entity_ref: HashMap<Entity, u32>,
}

type VisualChangedQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static RefVisual, Option<&'static ChildOf>),
    (
        crate::lights::NotLightSource,
        Without<nvf_play::DrawOnly>,
        Or<(Added<RefVisual>, Changed<RefVisual>, Added<ChildOf>, Changed<ChildOf>)>,
    ),
>;

type VisualExcludedAddedQuery<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<RefVisual>,
        Or<(
            Added<nvf_play::DrawOnly>,
            Added<PointLight>,
            Added<SpotLight>,
            Added<crate::lights::SessionLight>,
        )>,
    ),
>;

#[derive(bevy::ecs::system::SystemParam)]
struct VisualChanges<'w, 's> {
    changed: VisualChangedQuery<'w, 's>,
    excluded_added: VisualExcludedAddedQuery<'w, 's>,
    children: Query<'w, 's, &'static Children>,
    removed_visual: RemovedComponents<'w, 's, RefVisual>,
    removed_parent: RemovedComponents<'w, 's, ChildOf>,
    removed_draw_only: RemovedComponents<'w, 's, nvf_play::DrawOnly>,
    removed_point_light: RemovedComponents<'w, 's, PointLight>,
    removed_spot_light: RemovedComponents<'w, 's, SpotLight>,
    removed_session_light: RemovedComponents<'w, 's, crate::lights::SessionLight>,
}

impl VisualRootCache {
    fn is_root(v: u32, p: Option<&ChildOf>, visuals: &Visuals) -> bool {
        p.is_none_or(|p| visuals.get(p.parent()).map_or(true, |(_, pv, _)| pv.0 != v))
    }

    fn remove_entity(&mut self, e: Entity) -> Option<u32> {
        let old = self.entity_ref.remove(&e)?;
        if let Some(list) = self.roots.get_mut(&old) {
            list.retain(|&x| x != e);
            if list.is_empty() {
                self.roots.remove(&old);
            }
        }
        Some(old)
    }

    fn insert_root(&mut self, r: u32, e: Entity) {
        if let Some(old) = self.entity_ref.insert(e, r) {
            if old != r {
                if let Some(list) = self.roots.get_mut(&old) {
                    list.retain(|&x| x != e);
                    if list.is_empty() {
                        self.roots.remove(&old);
                    }
                }
            }
        }
        let list = self.roots.entry(r).or_default();
        if !list.contains(&e) {
            list.push(e);
        }
    }

    fn revalidate_ref(&mut self, r: u32, visuals: &Visuals) {
        let Some(mut list) = self.roots.remove(&r) else { return };
        list.retain(|&e| {
            let keep = visuals.get(e).ok().is_some_and(|(_, v, p)| v.0 == r && Self::is_root(r, p, visuals));
            if !keep && self.entity_ref.get(&e) == Some(&r) {
                self.entity_ref.remove(&e);
            }
            keep
        });
        if !list.is_empty() {
            self.roots.insert(r, list);
        }
    }

    fn refresh_entity(&mut self, e: Entity, visuals: &Visuals) {
        match visuals.get(e) {
            Ok((_, v, p)) if v.0 != FormId::PLAYER_REF.0 => {
                if Self::is_root(v.0, p, visuals) {
                    self.insert_root(v.0, e);
                } else {
                    self.remove_entity(e);
                }
                self.revalidate_ref(v.0, visuals);
            }
            _ => {
                self.remove_entity(e);
            }
        }
    }

    /// Re-evaluate direct children of `parent` whenever `parent` loses `RefVisual`, gains an
    /// exclusion component (`DrawOnly`/`PointLight`/`SpotLight`/`SessionLight`), or mutates
    /// its `RefVisual` so any direct child carrying the old reference is promoted to root.
    fn promote_children(&mut self, parent: Entity, visuals: &Visuals, children: &Query<&Children>) {
        let Ok(kids) = children.get(parent) else { return };
        for child in kids.iter() {
            if visuals.get(child).is_ok() {
                self.refresh_entity(child, visuals);
            }
        }
    }

    fn drain_removed(changes: &mut VisualChanges) {
        for _ in changes.removed_visual.read() {}
        for _ in changes.removed_parent.read() {}
        for _ in changes.removed_draw_only.read() {}
        for _ in changes.removed_point_light.read() {}
        for _ in changes.removed_spot_light.read() {}
        for _ in changes.removed_session_light.read() {}
    }

    /// Idle-frame maintenance when `rt` is `None` or nothing wants a reference:
    /// drain `RemovedComponents` readers without scanning `Added`/`Changed` queries
    /// and reset `initialized` so the next active frame rebuilds the cache once.
    fn suspend(&mut self, changes: &mut VisualChanges) {
        Self::drain_removed(changes);
        self.initialized = false;
        self.roots.clear();
        self.entity_ref.clear();
    }

    fn sync(&mut self, visuals: &Visuals, changes: &mut VisualChanges) {
        if !self.initialized {
            // Drain any initial deltas before doing the one-time full scan.
            Self::drain_removed(changes);
            self.roots.clear();
            self.entity_ref.clear();
            for (e, v, p) in visuals.iter() {
                if v.0 != FormId::PLAYER_REF.0 && Self::is_root(v.0, p, visuals) {
                    self.insert_root(v.0, e);
                }
            }
            self.initialized = true;
            return;
        }

        // 1. Removals / despawns of `RefVisual`: evict and promote any direct children
        //    that kept `RefVisual` if the parent entity itself survived.
        for e in changes.removed_visual.read() {
            self.remove_entity(e);
            self.promote_children(e, visuals, &changes.children);
        }

        // 2. Added exclusion components (`DrawOnly`, `PointLight`, `SpotLight`, `SessionLight`):
        //    evict the excluded entity and promote any direct `RefVisual` children.
        for e in changes.excluded_added.iter() {
            self.remove_entity(e);
            self.promote_children(e, visuals, &changes.children);
        }

        // 3. Removals of `ChildOf`, `DrawOnly`, `PointLight`, `SpotLight`, `SessionLight`
        //    can make an existing `RefVisual` entity become a valid root (or shadow a child).
        for e in changes.removed_parent.read() {
            self.refresh_entity(e, visuals);
        }
        for e in changes.removed_draw_only.read() {
            self.refresh_entity(e, visuals);
        }
        for e in changes.removed_point_light.read() {
            self.refresh_entity(e, visuals);
        }
        for e in changes.removed_spot_light.read() {
            self.refresh_entity(e, visuals);
        }
        for e in changes.removed_session_light.read() {
            self.refresh_entity(e, visuals);
        }

        // 4. Added or mutated `RefVisual` / `ChildOf`: update the entity and promote direct
        //    children in case `RefVisual` changed from `old_r` to `new_r` or `PLAYER_REF`.
        for (e, v, p) in changes.changed.iter() {
            if v.0 == FormId::PLAYER_REF.0 {
                self.remove_entity(e);
            } else if Self::is_root(v.0, p, visuals) {
                self.insert_root(v.0, e);
                self.revalidate_ref(v.0, visuals);
            } else {
                self.remove_entity(e);
                self.revalidate_ref(v.0, visuals);
            }
            self.promote_children(e, visuals, &changes.children);
        }
    }

    /// Look up the live root entity for `r`, defensively pruning any stale entry.
    fn get_live(&mut self, r: u32, visuals: &Visuals) -> Option<Entity> {
        let list = self.roots.get_mut(&r)?;
        while let Some(&e) = list.first() {
            if visuals.get(e).ok().is_some_and(|(_, v, p)| v.0 == r && Self::is_root(r, p, visuals)) {
                return Some(e);
            }
            list.remove(0);
            if self.entity_ref.get(&e) == Some(&r) {
                self.entity_ref.remove(&e);
            }
        }
        self.roots.remove(&r);
        None
    }
}

/// A node of a model file: its name, its parent's index, its local transform.
#[derive(Clone)]
struct FileNode {
    name: String,
    parent: Option<usize>,
    translation: Vec3,
    rotation: Quat,
    scale: f32,
}

/// A model file's nodes and shapes (under meshes), depth first from its root, children in
/// stored order, as `walk` tells drawn ones.
/// A reference's node tree from its model file's nodes (`file`): names (a repeated name
/// as "name:0", "name:1" …), parents and local transforms, with the scripts' held
/// transforms and materials over them.
fn file_scene(
    r: FormId,
    file: &[FileNode],
    table: &HashMap<String, [f32; 9]>,
    held: &HashMap<(u32, ModelView, String), Held>,
    materials: &HashMap<(u32, ModelView, String), [f32; 9]>,
) -> Vec<SceneNode> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    file.iter()
        .map(|f| {
            let n = seen.entry(f.name.to_ascii_lowercase()).or_insert(0);
            let shown = if *n == 0 { f.name.clone() } else { format!("{}:{}", f.name, *n - 1) };
            *n += 1;
            let h = held.get(&(r.0, ModelView::Reference, shown.clone())).copied().unwrap_or_default();
            let (z, y, x) = h.rotation.unwrap_or(f.rotation).to_euler(EulerRot::ZYX);
            let material = materials.get(&(r.0, ModelView::Reference, shown.clone())).or_else(|| table.get(&f.name)).copied();
            SceneNode {
                name: shown,
                parent: f.parent,
                translation: h.translation.unwrap_or(f.translation).to_array(),
                rotation: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                scale: h.scale.unwrap_or(f.scale),
                flags: 0,
                material,
                info: None,
            }
        })
        .collect()
}

fn file_tree(data: &GameData, model: &str) -> Vec<FileNode> {
    let mut out = Vec::new();
    if model.is_empty() {
        return out;
    }
    let Some(bytes) = data.read(&format!("meshes\\{model}")) else { return out };
    let Ok(file) = fnv_nif::NifFile::parse(&bytes, Some(&data.schema)) else { return out };
    let dec = fnv_nif::scene::Decoded::new(&file, &bytes, &data.schema);
    let objects = fnv_nif::scene::scene_objects(&file, &dec);
    let by_block: HashMap<usize, &fnv_nif::scene::SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let mut stack: Vec<(usize, Option<usize>)> = objects.iter().filter(|o| o.parent.is_none()).map(|o| (o.block, None)).rev().collect();
    let mut seen = std::collections::HashSet::new();
    while let Some((b, parent)) = stack.pop() {
        let Some(o) = by_block.get(&b).filter(|_| seen.insert(b)) else { continue };
        let t = &o.transform;
        // The stored rows taken as columns: the angles the original reads of a file's node
        // [observed: GetNifBlockRotation of the 10mm pistol's ProjectileNode, 90 0 90, is
        // this matrix's; the transpose nvf_assets::nif_spawn draws with gives its inverse].
        let m = Mat3::from_cols_array(&t.rotation);
        let at = out.len();
        out.push(FileNode { name: o.name.clone().unwrap_or_default(), parent, translation: Vec3::from_array(t.translation), rotation: Quat::from_mat3(&m), scale: t.scale });
        stack.extend(o.children.iter().rev().map(|&c| (c, Some(at))));
    }
    out
}

/// A shape's alpha (flags, threshold) and stencil (flags, reference, mask) properties.
type Props = (Option<[f32; 2]>, Option<[f32; 3]>);

/// Each named shape's NiAlphaProperty and NiStencilProperty values in a model file (under
/// meshes), for JohnnyGuitar's Get…PropertyValue [documented: nif.xml's fields;
/// hypothesis: the functions' item order is the fields'].
fn property_table(data: &GameData, model: &str) -> HashMap<String, Props> {
    let mut out: HashMap<String, Props> = HashMap::new();
    if model.is_empty() {
        return out;
    }
    let Some(bytes) = data.read(&format!("meshes\\{model}")) else { return out };
    let Ok(file) = fnv_nif::NifFile::parse(&bytes, Some(&data.schema)) else { return out };
    let dec = fnv_nif::scene::Decoded::new(&file, &bytes, &data.schema);
    for i in 0..file.blocks.len() {
        let Some(s) = dec.get(i as i32) else { continue };
        let Some(name) = s.text("name").filter(|n| !n.is_empty()) else { continue };
        for &r in s.refs("properties").unwrap_or_default() {
            let (Some(b), Some(p)) = (file.blocks.get(r as usize), dec.get(r)) else { continue };
            let int = |k: &str| p.int(k).map(|v| v as f32);
            let e = out.entry(name.to_string()).or_default();
            match b.type_name.as_str() {
                "NiAlphaProperty" => e.0 = Some([int("flags").unwrap_or(0.0), int("threshold").unwrap_or(0.0)]),
                "NiStencilProperty" => {
                    let reference = int("stencil_ref").or_else(|| int("stencil_reference")).unwrap_or(0.0);
                    let mask = int("stencil_mask").unwrap_or(0.0);
                    e.1 = Some([int("flags").unwrap_or(0.0), reference, mask]);
                }
                _ => {}
            }
        }
    }
    out
}

/// The world-space bounding sphere of `node` and what lies under it: of its shapes'
/// boxes, or the node's own place (radius 0) without any.
fn node_bound(at: usize, tree: &[(Entity, Option<usize>)], globals: &Query<&GlobalTransform>, boxes: &Query<&Aabb>) -> Option<[f32; 4]> {
    let mut spheres: Vec<(Vec3, f32)> = Vec::new();
    for (k, (e, _)) in tree.iter().enumerate() {
        // k under `at` (or `at` itself).
        let mut up = Some(k);
        let mut under = false;
        while let Some(i) = up {
            if i == at {
                under = true;
                break;
            }
            up = tree[i].1;
        }
        if !under {
            continue;
        }
        if let (Ok(b), Ok(g)) = (boxes.get(*e), globals.get(*e)) {
            let c = g.transform_point(Vec3::from(b.center));
            let s = g.compute_transform().scale.abs().max_element();
            spheres.push((c, Vec3::from(b.half_extents).length() * s));
        }
    }
    let own = globals.get(tree.get(at)?.0).ok()?.translation();
    let (centre, radius) = if spheres.is_empty() {
        (own, 0.0)
    } else {
        let (lo, hi) = spheres.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), (c, r)| (lo.min(*c - *r), hi.max(*c + *r)));
        let mid = (lo + hi) / 2.0;
        (mid, spheres.iter().map(|(c, r)| c.distance(mid) + r).fold(0.0, f32::max))
    };
    let g = nvf_play::space::to_game(centre);
    Some([g[0], g[1], g[2], radius])
}

/// What the mirror reads beside the nodes: their places in the world and boxes (bounds),
/// and the particle systems.
/// With the simulation: the played world, where the references stand (outside a game turn
/// the game's own world is a stand-in, `nvf_play::script::with_world`).
type Extra<'w, 's> = (Query<'w, 's, &'static GlobalTransform>, Query<'w, 's, &'static Aabb>, Option<ResMut<'w, crate::particles::Particles>>, Option<Res<'w, crate::world::WorldData>>, Option<Res<'w, nvf_play::sim::SimState>>);

/// Each named shape's NiMaterialProperty values in a model file (under meshes), in
/// GetMaterialPropertyValue's order: specular R G B, emissive R G B, glossiness, alpha,
/// emissive mult [documented: the page's value IDs; the fields: nif.xml
/// NiMaterialProperty]. The emissive mult is 1 in files without it.
fn material_table(data: &GameData, model: &str) -> HashMap<String, [f32; 9]> {
    let mut out = HashMap::new();
    if model.is_empty() {
        return out;
    }
    let Some(bytes) = data.read(&format!("meshes\\{model}")) else { return out };
    let Ok(file) = fnv_nif::NifFile::parse(&bytes, Some(&data.schema)) else { return out };
    let dec = fnv_nif::scene::Decoded::new(&file, &bytes, &data.schema);
    for i in 0..file.blocks.len() {
        let Some(s) = dec.get(i as i32) else { continue };
        let Some(name) = s.text("name").filter(|n| !n.is_empty()) else { continue };
        for &r in s.refs("properties").unwrap_or_default() {
            if !file.blocks.get(r as usize).is_some_and(|b| b.type_name == "NiMaterialProperty") {
                continue;
            }
            let Some(m) = dec.get(r) else { continue };
            let c = |f: &str| m.floats(f).filter(|v| v.len() >= 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
            let (sp, em) = (c("specular_color"), c("emissive_color"));
            let v = [sp[0], sp[1], sp[2], em[0], em[1], em[2], m.float("glossiness").unwrap_or(0.0), m.float("alpha").unwrap_or(1.0), m.float("emissive_mult").unwrap_or(1.0)];
            out.entry(name.to_string()).or_insert(v);
        }
    }
    out
}

/// A shape's material drawn with these values: the specular tint, the emitted light
/// (colour times mult), the roughness of the glossiness, the alpha [hypothesis: the
/// spawn's own display tuning of the emitted light is not redone here]. `changed` is the
/// index of the value the script set (the others are written back as they were). On a
/// material shaded from an authored set (#2981) the glossiness and the specular colour go to
/// its legacy look, so an edit of another value leaves the set's roughness and highlight as
/// they are, and the scripted values apply if the set comes off
/// (`nvf_assets::authored::set_legacy_roughness`, `set_legacy_specular_tint`); a specular
/// colour the script itself set is drawn at once.
fn set_material(m: &mut NifMaterial, v: &[f32; 9], changed: usize) {
    nvf_assets::authored::set_legacy_specular_tint(m, Color::linear_rgb(v[0], v[1], v[2]), changed < 3);
    m.base.emissive = LinearRgba::rgb(v[3] * v[8], v[4] * v[8], v[5] * v[8]);
    nvf_assets::authored::set_legacy_roughness(m, nvf_assets::nif_spawn::gloss_roughness(v[6]));
    m.base.base_color.set_alpha(v[7].clamp(0.0, 1.0));
}

/// Apply live `NiAlphaProperty` values (`[flags, threshold]`) to a shape's private
/// `NifMaterial` via the engine's canonical `nif_spawn::alpha_mode` mapping [design].
fn set_alpha_property(m: &mut NifMaterial, v: &[f32; 2]) {
    m.base.alpha_mode = nvf_assets::nif_spawn::alpha_mode(v[0].round() as i64, v[1].round() as i64);
}

/// Apply live `NiStencilProperty` draw-mode bits (`flags` bits 10-11) to a shape's
/// private `NifMaterial` [documented: nif.xml StencilDrawMode; design: Bevy's
/// `StandardMaterial` does not expose per-material hardware stencil ref/mask].
fn set_stencil_property(m: &mut NifMaterial, v: &[f32; 3]) {
    let (double_sided, cull_mode) = nvf_assets::nif_spawn::stencil_cull(v[0].round() as i64);
    m.base.double_sided = double_sided;
    m.base.cull_mode = cull_mode;
}

/// Shape material components queried by [`mirror`] and [`mutate_shape_material`].
type DrawnMaterials<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static MeshMaterial3d<NifMaterial>>,
        Option<&'static crate::mesh_terrain_blend::BlendShape>,
        Option<&'static nvf_assets::nif_material::MaterialOverride>,
        Option<&'static nvf_play::fade::Fading>,
    ),
>;

/// Snapshot of a live particle system's name, emitter object name, and script values.
type EmitterSummary = (String, Option<String>, [f32; 21]);

/// Clone-on-write a shape entity's `NifMaterial` (sharing one private copy across
/// multiple edits in the same frame via `made_now` / `owned`) and run `mutate`.
/// While a fade or `SetActorAlpha` owns a see-through copy (`nvf_play::fade::Fading`),
/// edits target the mesh's own persistent material and refresh the see-through copy in
/// place so the edit survives when the fade ends [design, #3105].
#[allow(clippy::too_many_arguments)]
fn mutate_shape_material(
    e: Entity,
    drawn: &DrawnMaterials,
    assets: Option<&mut Assets<NifMaterial>>,
    owned: &mut HashMap<Entity, Handle<NifMaterial>>,
    made_now: &mut HashMap<Entity, Handle<NifMaterial>>,
    commands: &mut Commands,
    mutate: impl FnOnce(&mut NifMaterial),
) {
    let Some(assets) = assets else { return };
    let Ok((handle, blend, drawing, fading)) = drawn.get(e) else { return };
    let Some(handle) = nvf_assets::nif_surface_blend::source_material(handle, blend) else { return };
    // A private copy made earlier in this drain is not drawn yet (deferred
    // insert): a later edit of the same shape goes to that copy, or a second
    // copy of the old material would win with the first edit lost (#2981).
    let source = match made_now.get(&e) {
        Some(h) => h.clone(),
        None => nvf_play::fade::persistent_material(handle, drawing, fading).clone(),
    };
    let h = match owned.get(&e).filter(|h| **h == source).cloned() {
        Some(h) => {
            if let Some(mut m) = assets.get_mut(&h) {
                mutate(&mut m);
            }
            h
        }
        None => {
            let Some(mut m) = assets.get(&source).cloned() else { return };
            mutate(&mut m);
            let h = assets.add(m);
            owned.insert(e, h.clone());
            made_now.insert(e, h.clone());
            h
        }
    };
    if fading.is_some() {
        nvf_play::fade::queue_retarget(commands, e, h);
    } else {
        commands.entity(e).insert(MeshMaterial3d(h));
    }
}

/// A node transform a script set, held over the animation.
#[derive(Clone, Copy, Default)]
struct Held {
    translation: Option<Vec3>,
    rotation: Option<Quat>,
    scale: Option<f32>,
}

#[derive(Default)]
struct PendingHeld {
    model: Option<u64>,
    value: OrderedHeld,
}

/// Per-property chronology matters when different pending names later resolve to
/// one node, or an old pending request meets a newer already-bound request.
#[derive(Clone, Copy, Default)]
struct OrderedHeld {
    value: Held,
    order: [u64; 3],
}

/// Draw-only entities (the full-detail ring, `nvf_play::DrawOnly`) are not the game's: the
/// scripts' NIF functions never find them.
pub(crate) type Visuals<'w, 's> = Query<'w, 's, (Entity, &'static RefVisual, Option<&'static ChildOf>), (crate::lights::NotLightSource, Without<nvf_play::DrawOnly>)>;

/// Distinguish an explicit NIF rename from factory entity labels on actor roots.
/// The marker dies with the entity; it is not a FormId/name override for a new rig.
#[derive(Component)]
pub(crate) struct RenamedNifNode;

pub(crate) type NodeQuery<'w, 's> = Query<'w, 's, (Option<&'static Name>, &'static mut Transform, Option<&'static Children>, Has<NifHidden>, Has<RenamedNifNode>, Option<&'static crate::nif_attachments::AttachedNode>)>;

/// The actors' rigs and both built player models.
pub(crate) type Rigs<'w, 's> = (Query<'w, 's, (Entity, &'static ActorRig)>, Option<Res<'w, crate::player_body::PlayerBody>>);

pub(crate) fn concrete_view(r: u32, view: ModelView) -> ModelView {
    if r != FormId::PLAYER_REF.0 { ModelView::Reference }
    else if view == ModelView::Reference { ModelView::ThirdPerson }
    else { view }
}

fn selected_player_rig<T>(view: ModelView, third: impl FnOnce() -> Option<T>, first: impl FnOnce() -> Option<T>) -> Option<T> {
    if view == ModelView::FirstPerson { first() } else { third() }
}

/// A reference's rig: its root entity and the rig.
fn rig_of(r: u32, view: ModelView, rigs: &Rigs) -> Option<(Entity, ActorRig)> {
    let (actors, player) = rigs;
    if r == FormId::PLAYER_REF.0 {
        // Player callers resolve the current-view sentinel before selecting a rig.
        if view == ModelView::Reference { return None; }
        return player.as_deref().and_then(|p| selected_player_rig(view, || p.script_rig(false), || p.script_rig(true)));
    }
    actors.iter().find(|(_, a)| a.reference == r).map(|(e, a)| (e, a.clone()))
}

/// The root entity of a reference's drawing.
pub(crate) fn root_of(r: u32, view: ModelView, rigs: &Rigs, visuals: &Visuals) -> Option<Entity> {
    if let Some((e, _)) = rig_of(r, view, rigs) {
        return Some(e);
    }
    if r == FormId::PLAYER_REF.0 { return None; }
    visuals.iter().filter(|(_, v, _)| v.0 == r).find(|(_, _, p)| p.is_none_or(|p| visuals.get(p.parent()).map_or(true, |(_, v, _)| v.0 != r))).map(|(e, _, _)| e)
}

/// The node entities under `root`, depth first, with their parent's index; an actor's
/// root first (its skeleton's root node), the rest under it.
fn walk(root: Entity, nodes: &NodeQuery, rig: &RigNames) -> Vec<(Entity, Option<usize>)> {
    let mut out = Vec::new();
    let mut stack: Vec<(Entity, Option<usize>)> = Vec::new();
    let top = if rig.root == Some(root) {
        out.push((root, None));
        Some(0)
    } else {
        None
    };
    if let Ok((_, _, Some(ch), _, _, _)) = nodes.get(root) {
        stack.extend(ch.iter().rev().map(|c| (c, top)));
    }
    while let Some((e, parent)) = stack.pop() {
        let at = out.len();
        out.push((e, parent));
        if let Ok((_, _, Some(ch), _, _, _)) = nodes.get(e) {
            stack.extend(ch.iter().rev().map(|c| (c, Some(at))));
        }
    }
    out
}

/// An actor's skeleton node names, by entity (its node entities carry no `Name`), and its
/// root entity (named by the skeleton, not by the `Name` the spawn gives it).
#[derive(Default)]
pub(crate) struct RigNames {
    root: Option<Entity>,
    names: HashMap<Entity, String>,
}

impl RigNames {
    pub(crate) fn is_root(&self, e: Entity) -> bool {
        self.root == Some(e)
    }

    pub(crate) fn name_of(&self, e: Entity) -> Option<&str> {
        self.names.get(&e).map(String::as_str)
    }
}

pub(crate) fn rig_names(r: u32, view: ModelView, rigs: &Rigs) -> RigNames {
    rig_of(r, view, rigs)
        .map(|(root, a)| RigNames { root: Some(root), names: a.nodes.iter().zip(&a.skel.names).filter_map(|(e, n)| n.clone().map(|n| (*e, n))).collect() })
        .unwrap_or_default()
}

/// A node's name: an actor root's from its skeleton, else its `Name`, else the rig's.
fn name_of(e: Entity, nodes: &NodeQuery, rig: &RigNames) -> String {
    let node = nodes.get(e).ok();
    if let Some(name) = node.as_ref().filter(|n| n.4).and_then(|n| n.0) {
        return name.as_str().to_string();
    }
    if rig.root == Some(e) {
        if let Some(n) = rig.names.get(&e) {
            return n.clone();
        }
    }
    node.and_then(|n| n.0.map(|n| n.as_str().to_string())).or_else(|| rig.names.get(&e).cloned()).unwrap_or_default()
}

/// The node of that name under `root`: exactly, else without regard to case.
pub(crate) fn find(root: Entity, name: &str, nodes: &NodeQuery, rig: &RigNames) -> Option<Entity> {
    let all = walk(root, nodes, rig);
    all.iter().map(|(e, _)| *e).find(|&e| name_of(e, nodes, rig) == name).or_else(|| all.iter().map(|(e, _)| *e).find(|&e| name_of(e, nodes, rig).eq_ignore_ascii_case(name)))
}

type HeldModels = HashMap<(u32, ModelView, String), Held>;

fn remember_transform(view: ModelView, edit: &SceneEdit, held: &mut HeldModels) -> bool {
    match edit {
        SceneEdit::Translation { r, node, v } => {
            held.entry((r.0, view, node.clone())).or_default().translation = Some(Vec3::from(*v));
        }
        SceneEdit::Rotation { r, node, v } => {
            let [x, y, z] = v.map(f32::to_radians);
            held.entry((r.0, view, node.clone())).or_default().rotation = Some(Quat::from_euler(EulerRot::ZYX, z, y, x));
        }
        SceneEdit::Scale { r, node, v } => {
            held.entry((r.0, view, node.clone())).or_default().scale = Some(*v);
        }
        _ => return false,
    }
    true
}

/// Resolve an opaque published identity only inside the actual current model.
/// Entity bits include the generation; live query membership rejects stale rig
/// selections, and ancestry rejects a node moved outside this model subtree.
pub(crate) fn bound_entity(root: Entity, identity: SceneIdentity, nodes: &NodeQuery, rig: &RigNames) -> Option<Entity> {
    if identity.model != root.to_bits() || nodes.get(root).is_err() { return None; }
    walk(root, nodes, rig).into_iter().map(|(e, _)| e)
        .find(|e| e.to_bits() == identity.node && nodes.get(*e).is_ok())
}

fn hold_value(edit: &SceneEdit, held: &mut OrderedHeld, order: u64) -> bool {
    match edit {
        SceneEdit::Translation { v, .. } => {
            held.value.translation = Some(Vec3::from(*v));
            held.order[0] = order;
        }
        SceneEdit::Rotation { v, .. } => {
            let [x, y, z] = v.map(f32::to_radians);
            held.value.rotation = Some(Quat::from_euler(EulerRot::ZYX, z, y, x));
            held.order[1] = order;
        }
        SceneEdit::Scale { v, .. } => {
            held.value.scale = Some(*v);
            held.order[2] = order;
        }
        _ => return false,
    }
    true
}

fn merge_held(target: &mut OrderedHeld, source: OrderedHeld) {
    if source.value.translation.is_some() && source.order[0] > target.order[0] {
        target.value.translation = source.value.translation;
        target.order[0] = source.order[0];
    }
    if source.value.rotation.is_some() && source.order[1] > target.order[1] {
        target.value.rotation = source.value.rotation;
        target.order[1] = source.order[1];
    }
    if source.value.scale.is_some() && source.order[2] > target.order[2] {
        target.value.scale = source.value.scale;
        target.order[2] = source.order[2];
    }
}

/// A held rotation is in the scripts' angles, whose rotation is the transpose (the
/// inverse) of the one a node is drawn with (`fnv_game::nif_blocks::SceneNode::rotation`).
fn apply_value(entity: Entity, held: Held, nodes: &mut NodeQuery) {
    if let Ok(mut t) = nodes.get_mut(entity) {
        if let Some(v) = held.translation { t.1.translation = v; }
        if let Some(q) = held.rotation { t.1.rotation = q.inverse(); }
        if let Some(s) = held.scale { t.1.scale = Vec3::splat(s); }
    }
}

fn apply_held(root: Entity, node: &str, held: Held, nodes: &mut NodeQuery, rig: &RigNames) {
    if let Some(mut t) = find(root, node, nodes, rig).and_then(|e| nodes.get_mut(e).ok()) {
        if let Some(v) = held.translation { t.1.translation = v; }
        if let Some(q) = held.rotation { t.1.rotation = q.inverse(); }
        if let Some(s) = held.scale { t.1.scale = Vec3::splat(s); }
    }
}

/// The material side of [`mirror`]: the data files, the shapes' materials, the assets.
type Materials<'w, 's> = (Option<Res<'w, GameData>>, DrawnMaterials<'w, 's>, Option<ResMut<'w, Assets<NifMaterial>>>);

/// The roots of created items (`nvf_play::dropped`) and the entities with children: an
/// item whose shapes hang straight from its root is drawn flat.
type Dropped<'w, 's> = (Query<'w, 's, (&'static nvf_play::dropped::DroppedRoot, Option<&'static nvf_play::dropped::DroppedModel>, Option<&'static Children>)>, Query<'w, 's, (), With<Children>>);

/// Per-frame O(1) lookup index for [`mirror_game`], built with a single scan over
/// `rigs`, `dropped`, and `visuals` instead of rescanning every `RefVisual` for each
/// watched or edited reference (`O(V + R + D + W)` instead of `O(W * V)`).
#[derive(Default)]
struct MirrorIndex<'a> {
    player_third: Option<(Entity, ActorRig)>,
    player_first: Option<(Entity, ActorRig)>,
    rigs_by_ref: HashMap<u32, (Entity, &'a ActorRig)>,
    visual_roots: HashMap<u32, Entity>,
    flat_by_ref: HashMap<u32, String>,
}

impl<'a> MirrorIndex<'a> {
    fn build_from_cache(
        wanted_refs: &HashSet<u32>,
        rigs: &'a Rigs,
        visuals: &Visuals,
        (roots, nested): &Dropped,
        cache: &mut VisualRootCache,
    ) -> Self {
        if wanted_refs.is_empty() {
            return Self::default();
        }
        let (actors, player) = rigs;
        let (player_third, player_first) = if wanted_refs.contains(&FormId::PLAYER_REF.0) {
            match player.as_deref() {
                Some(p) => (p.script_rig(false), p.script_rig(true)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        let mut rigs_by_ref = HashMap::new();
        for (e, a) in actors.iter() {
            if wanted_refs.contains(&a.reference) {
                rigs_by_ref.entry(a.reference).or_insert((e, a));
            }
        }
        let mut flat_by_ref = HashMap::new();
        for (d, m, ch) in roots.iter() {
            if wanted_refs.contains(&d.0)
                && !flat_by_ref.contains_key(&d.0)
                && ch.is_some_and(|c| c.iter().all(|e| !nested.contains(e)))
            {
                let m = m.map(|m| m.0.as_str()).unwrap_or_default();
                let m = m.get(..7).filter(|p| p.eq_ignore_ascii_case("meshes\\")).map_or(m, |_| &m[7..]).to_string();
                flat_by_ref.insert(d.0, m);
            }
        }
        let mut visual_roots = HashMap::new();
        for &r in wanted_refs {
            if r == FormId::PLAYER_REF.0 || rigs_by_ref.contains_key(&r) {
                continue;
            }
            if let Some(e) = cache.get_live(r, visuals) {
                visual_roots.insert(r, e);
            }
        }
        Self { player_third, player_first, rigs_by_ref, visual_roots, flat_by_ref }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn build(
        wanted_refs: &HashSet<u32>,
        rigs: &'a Rigs,
        visuals: &Visuals,
        (roots, nested): &Dropped,
    ) -> Self {
        if wanted_refs.is_empty() {
            return Self::default();
        }
        let (actors, player) = rigs;
        let (player_third, player_first) = if wanted_refs.contains(&FormId::PLAYER_REF.0) {
            match player.as_deref() {
                Some(p) => (p.script_rig(false), p.script_rig(true)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        let mut rigs_by_ref = HashMap::new();
        for (e, a) in actors.iter() {
            if wanted_refs.contains(&a.reference) {
                rigs_by_ref.entry(a.reference).or_insert((e, a));
            }
        }
        let mut flat_by_ref = HashMap::new();
        for (d, m, ch) in roots.iter() {
            if wanted_refs.contains(&d.0)
                && !flat_by_ref.contains_key(&d.0)
                && ch.is_some_and(|c| c.iter().all(|e| !nested.contains(e)))
            {
                // The drop's path is under meshes already; the tables here take it without.
                let m = m.map(|m| m.0.as_str()).unwrap_or_default();
                let m = m.get(..7).filter(|p| p.eq_ignore_ascii_case("meshes\\")).map_or(m, |_| &m[7..]).to_string();
                flat_by_ref.insert(d.0, m);
            }
        }
        let needs_visuals = wanted_refs
            .iter()
            .any(|&r| r != FormId::PLAYER_REF.0 && !rigs_by_ref.contains_key(&r));
        let mut visual_roots = HashMap::new();
        if needs_visuals {
            for (e, v, p) in visuals.iter() {
                if !wanted_refs.contains(&v.0)
                    || v.0 == FormId::PLAYER_REF.0
                    || rigs_by_ref.contains_key(&v.0)
                    || visual_roots.contains_key(&v.0)
                {
                    continue;
                }
                if p.is_none_or(|p| visuals.get(p.parent()).map_or(true, |(_, pv, _)| pv.0 != v.0)) {
                    visual_roots.insert(v.0, e);
                }
            }
        }
        Self { player_third, player_first, rigs_by_ref, visual_roots, flat_by_ref }
    }

    fn rig_of(&self, r: u32, view: ModelView) -> Option<(Entity, &ActorRig)> {
        if r == FormId::PLAYER_REF.0 {
            if view == ModelView::Reference { return None; }
            return selected_player_rig(
                view,
                || self.player_third.as_ref().map(|(e, a)| (*e, a)),
                || self.player_first.as_ref().map(|(e, a)| (*e, a)),
            );
        }
        self.rigs_by_ref.get(&r).copied()
    }

    fn root_of(&self, r: u32, view: ModelView) -> Option<Entity> {
        if let Some((e, _)) = self.rig_of(r, view) {
            return Some(e);
        }
        if r == FormId::PLAYER_REF.0 { return None; }
        self.visual_roots.get(&r).copied()
    }

    fn rig_names(&self, r: u32, view: ModelView) -> RigNames {
        self.rig_of(r, view)
            .map(|(root, a)| RigNames {
                root: Some(root),
                names: a.nodes.iter().zip(&a.skel.names).filter_map(|(e, n)| n.clone().map(|n| (*e, n))).collect(),
            })
            .unwrap_or_default()
    }

    fn drawn_flat(&self, r: u32) -> Option<&str> {
        self.flat_by_ref.get(&r).map(String::as_str)
    }
}

fn edit_ref(edit: &SceneEdit) -> Option<FormId> {
    match edit {
        SceneEdit::Translation { r, .. }
        | SceneEdit::Rotation { r, .. }
        | SceneEdit::Scale { r, .. }
        | SceneEdit::Flags { r, .. }
        | SceneEdit::ChildFlags { r, .. }
        | SceneEdit::Rename { r, .. }
        | SceneEdit::Remove { r, .. }
        | SceneEdit::Insert { r, .. }
        | SceneEdit::Attach { r, .. }
        | SceneEdit::Light { r, .. }
        | SceneEdit::Material { r, .. }
        | SceneEdit::AlphaProperty { r, .. }
        | SceneEdit::StencilProperty { r, .. }
        | SceneEdit::Particle { r, .. } => Some(*r),
        SceneEdit::ForModel { edit, .. } => edit_ref(edit),
    }
}

/// The model of `r` when it is a created item drawn as flat shapes (no node entities).
#[cfg(test)]
fn drawn_flat(r: u32, (roots, nested): &Dropped) -> Option<String> {
    roots.iter().find(|(d, _, ch)| d.0 == r && ch.is_some_and(|c| c.iter().all(|e| !nested.contains(e)))).map(|(_, m, _)| {
        // The drop's path is under meshes already; the tables here take it without.
        let m = m.map(|m| m.0.as_str()).unwrap_or_default();
        m.get(..7).filter(|p| p.eq_ignore_ascii_case("meshes\\")).map_or(m, |_| &m[7..]).to_string()
    })
}

/// The shapes' meshes, for SUP's NifBlockGetVerticles.
type Shapes<'w, 's> = (Query<'w, 's, &'static Mesh3d>, Option<Res<'w, Assets<Mesh>>>);

#[allow(clippy::too_many_arguments)]
fn mirror(
    mut script: Option<ResMut<ScriptState>>,
    rigs: Rigs,
    visuals: Visuals,
    mut visual_changes: VisualChanges,
    parents: Query<&ChildOf>,
    nodes: NodeQuery,
    commands: Commands,
    mut mem: Local<Memory>,
    materials: Materials,
    shapes: Shapes,
    extra: Extra,
    dropped: Dropped,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
        mem.visual_cache.suspend(&mut visual_changes);
        return;
    };
    mirror_game_impl(
        &mut rt.game,
        rigs,
        visuals,
        parents,
        nodes,
        commands,
        &mut mem,
        materials,
        shapes,
        extra,
        dropped,
        Some(&mut visual_changes),
    );
}

/// The real mirror producer, separated from ScriptState acquisition so authored
/// CPU controls can exercise the PlayerBody-to-script-to-ECS pipeline without a
/// game-data loader or renderer. This is also the production mirror's implementation.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(test), allow(dead_code))]
fn mirror_game(
    game: &mut fnv_game::game::Game<'_>,
    rigs: Rigs,
    visuals: Visuals,
    parents: Query<&ChildOf>,
    nodes: NodeQuery,
    commands: Commands,
    mem: &mut Memory,
    materials: Materials,
    shapes: Shapes,
    extra: Extra,
    dropped: Dropped,
) {
    mirror_game_impl(game, rigs, visuals, parents, nodes, commands, mem, materials, shapes, extra, dropped, None);
}

#[allow(clippy::too_many_arguments)]
fn mirror_game_impl(
    game: &mut fnv_game::game::Game<'_>,
    rigs: Rigs,
    visuals: Visuals,
    parents: Query<&ChildOf>,
    mut nodes: NodeQuery,
    mut commands: Commands,
    mem: &mut Memory,
    (data, drawn, mut assets): Materials,
    (shapes, meshes): Shapes,
    (globals, boxes, mut particles, world, sim): Extra,
    dropped: Dropped,
    visual_changes: Option<&mut VisualChanges>,
) {
    let Memory { logged, held, bound_held, pending_held, edit_order, tables, materials, alpha_props, stencil_props, owned, attachment_materials, attachment_alpha, attachment_stencil, props, files, visual_cache } = mem;
    game.scene.use_separate_player_models();
    let edits = game.scene.take_targeted_edits();
    // Collect watched refs and wanted vertices upfront so a single per-frame pass
    // indexes rigs, flat drops, and RefVisual roots for every ref touched this frame.
    let mut watched: Vec<FormId> = game.scene.watched().collect();
    if !watched.is_empty() {
        let mut seen_watched: HashSet<u32> = watched.iter().map(|r| r.0).collect();
        let created = rigs.0.iter().map(|(_, a)| a.reference).filter(|r| r >> 24 == 0xFF).chain(dropped.0.iter().map(|(d, _, _)| d.0));
        for r in std::iter::once(FormId::PLAYER_REF.0).chain(created) {
            if seen_watched.insert(r) {
                watched.push(FormId(r));
            }
        }
    }
    let wanted: Vec<(FormId, String)> = game.scene.wanted_vertices().cloned().collect();
    let mut wanted_refs: HashSet<u32> = HashSet::with_capacity(
        edits.len() + held.len() + pending_held.len() + bound_held.len() + watched.len() + wanted.len(),
    );
    for queued in &edits {
        if let Some(r) = edit_ref(&queued.edit) {
            wanted_refs.insert(r.0);
        }
    }
    wanted_refs.extend(held.keys().map(|(r, _, _)| *r));
    wanted_refs.extend(pending_held.keys().map(|(r, _, _)| *r));
    wanted_refs.extend(bound_held.keys().map(|(r, _, _)| *r));
    wanted_refs.extend(watched.iter().map(|r| r.0));
    wanted_refs.extend(wanted.iter().map(|(r, _)| r.0));
    let index = if let Some(visual_changes) = visual_changes {
        if wanted_refs.is_empty() {
            visual_cache.suspend(visual_changes);
            MirrorIndex::default()
        } else {
            visual_cache.sync(&visuals, visual_changes);
            MirrorIndex::build_from_cache(&wanted_refs, &rigs, &visuals, &dropped, visual_cache)
        }
    } else {
        MirrorIndex::build(&wanted_refs, &rigs, &visuals, &dropped)
    };
    let mut edits_rest = Vec::new();
    for queued in edits {
        let (view, edit) = queued.edit.into_model();
        let Some(r) = edit_ref(&edit) else { continue };
        let view = concrete_view(r.0, view);
        // File-only publications deliberately keep their original legacy contract:
        // transforms are remembered even when no RefVisual root can be resolved,
        // and non-transforms still reach the original name-based handlers below.
        if queued.target == SceneTarget::Legacy && index.drawn_flat(r.0).is_some() {
            if !remember_transform(view, &edit, held) { edits_rest.push((view, edit, None)); }
            continue;
        }
        // Never wrap chronology and let an old entity request beat a later one.
        // The file-only legacy route above keeps its existing behavior.
        let Some(order) = edit_order.checked_add(1) else { continue };
        *edit_order = order;
        let root = index.root_of(r.0, view);
        if matches!(queued.target, SceneTarget::Legacy | SceneTarget::Model(_)) {
            let captured = match queued.target { SceneTarget::Model(model) => Some(model), _ => None };
            if captured.is_some_and(|model| root.is_none_or(|root| root.to_bits() != model || nodes.get(root).is_err())) {
                continue;
            }
            let name = match &edit {
                SceneEdit::Translation { node, .. } | SceneEdit::Rotation { node, .. }
                | SceneEdit::Scale { node, .. } => Some(node),
                _ => None,
            };
            if let Some(name) = name {
                let model = captured.or_else(|| root.filter(|root| nodes.get(*root).is_ok()).map(Entity::to_bits));
                let pending = pending_held.entry((r.0, view, name.clone())).or_default();
                // Acquiring the first root must retain the other TRS components
                // requested while cold. An explicit request in a new model replaces
                // a pending value from the old model; absence never unpins one.
                if let Some(model) = model {
                    if pending.model.is_some_and(|old| old != model) { pending.value = OrderedHeld::default(); }
                    pending.model = Some(model);
                }
                hold_value(&edit, &mut pending.value, order);
                continue;
            }
            // Main's prepublication structural edits are consume-once name lookups
            // on an already drawn root, never deferred destructive requests.
            if root.is_some() { edits_rest.push((view, edit, None)); }
            continue;
        }
        let Some(root) = root else { continue };
        let rig = index.rig_names(r.0, view);
        let entity = match queued.target {
            SceneTarget::Node(identity) => {
                let Some(entity) = bound_entity(root, identity, &nodes, &rig) else { continue };
                if matches!(&edit, SceneEdit::Translation { .. } | SceneEdit::Rotation { .. } | SceneEdit::Scale { .. }) {
                    hold_value(&edit, bound_held.entry((r.0, view, identity)).or_default(), order);
                    continue;
                }
                entity
            }
            SceneTarget::Legacy | SceneTarget::Model(_) | SceneTarget::Missing => continue,
        };
        edits_rest.push((view, edit, Some(entity)));
    }
    // The private material copies made in this drain, by shape (`SceneEdit::Material`).
    let mut made_now: HashMap<Entity, Handle<NifMaterial>> = HashMap::new();
    for (view, edit, entity) in edits_rest {
        let Some(r) = edit_ref(&edit) else { continue };
        let Some(root) = index.root_of(r.0, view) else { continue };
        let rig = index.rig_names(r.0, view);
        // Cold compatibility and the legacy file path resolve once by name/index. Keeping
        // an absent entity is significant: particle/material handlers historically
        // did work before or without finding a concrete shape.
        let entity = entity.or_else(|| match &edit {
            SceneEdit::ChildFlags { node, child, .. } => find(root, node, &nodes, &rig)
                .and_then(|p| nodes.get(p).ok().and_then(|(_, _, ch, _, _, _)| ch.and_then(|c| c.iter().nth(*child)))),
            SceneEdit::Insert { target, .. } | SceneEdit::Attach { target, .. } | SceneEdit::Light { target, .. } => find(root, target, &nodes, &rig),
            SceneEdit::Flags { node, .. } | SceneEdit::Rename { node, .. }
            | SceneEdit::Remove { node, .. } | SceneEdit::Material { node, .. }
            | SceneEdit::AlphaProperty { node, .. } | SceneEdit::StencilProperty { node, .. }
            | SceneEdit::Particle { node, .. } => find(root, node, &nodes, &rig),
            _ => None,
        });
        match edit {
            SceneEdit::ForModel { .. } => {}
            SceneEdit::Translation { .. } | SceneEdit::Rotation { .. } | SceneEdit::Scale { .. } => {}
            // Published ChildFlags captures the actual child identity; the cold
            // compatibility branch resolves its name/index once above.
            SceneEdit::Flags { flags, .. } | SceneEdit::ChildFlags { flags, .. } => {
                if let Some(entity) = entity {
                    if flags & 1 != 0 {
                        commands.entity(entity).insert((NifHidden, Visibility::Hidden));
                    } else {
                        commands.entity(entity).remove::<NifHidden>().insert(Visibility::Inherited);
                    }
                }
            }
            SceneEdit::Rename { node, to, .. } => {
                if let Some(entity) = entity {
                    commands.entity(entity).insert((Name::new(to.clone()), RenamedNifNode));
                    if let Some(value) = held.remove(&(r.0, view, node)) {
                        held.insert((r.0, view, to), value);
                    }
                }
            }
            SceneEdit::Remove { node, .. } => {
                if let Some(entity) = entity {
                    commands.entity(entity).despawn();
                    bound_held.retain(|(reference, model, identity), _| !(*reference == r.0 && *model == view && identity.node == entity.to_bits()));
                }
                held.remove(&(r.0, view, node));
            }
            SceneEdit::Insert { name, as_parent, .. } => {
                let Some(t) = entity else { continue };
                let new = commands.spawn((Name::new(name), Transform::IDENTITY, Visibility::Inherited)).id();
                if as_parent {
                    let above = parents.get(t).map(|p| p.parent()).unwrap_or(root);
                    commands.entity(above).add_child(new);
                    commands.entity(new).add_child(t);
                } else {
                    commands.entity(t).add_child(new);
                }
            }
            // The attachment producer consumed structural registration before this
            // mirror, with a deferred-command barrier before publication.
            SceneEdit::Attach { .. } => {}
            // JIP's AttachLight: a node of that name under the target at the offset, lit
            // by the LIGH's colour, brightness and radius as a muzzle flash's light is
            // (`crate::shot_fx`; docs/spec-notes/ext-unlisted-commands.md §2.1).
            SceneEdit::Light { target, name, light, offset, .. } => {
                let Some(t) = entity else { continue };
                let data = world.as_deref().and_then(|w| fnv_world::lighting::light_data(w.lo, w.schema, light.0));
                let new = commands.spawn((Name::new(name), Transform::from_translation(Vec3::from(offset)), Visibility::Inherited)).id();
                if let Some(l) = data.filter(|l| l.radius > 0.0) {
                    let color = crate::lights::display_color(l.color.map(|b| b as f32 / 255.0));
                    let intensity = crate::lights::lumens(l.fade.max(0.0), crate::sky::light_unit());
                    // It casts lamp shadows as a placed light does (`crate::light_shadows`).
                    commands.entity(new).insert((
                        PointLight { color, intensity, range: l.radius, shadow_maps_enabled: false, ..default() },
                        crate::light_shadows::ShadowSource(crate::light_shadows::SourceKind::Attached),
                    ));
                    info!("nif mirror: light {:08X} on {:08X}'s {target}, radius {:.0}", light.0, r.0, l.radius);
                }
                commands.entity(t).add_child(new);
            }
            // A placed or attached/overlay model's particle system of that name (or
            // emitting from a node of that name), changed while it lives (JohnnyGuitar's
            // SetNiPSysModifierValue / SetParticleEmitterSpawnRate). Attached nodes
            // mutate only their own instance's emitter, never a namesake placed system.
            SceneEdit::Particle { node, id, v, .. } => {
                let attached = entity.and_then(|e| nodes.get(e).ok()).and_then(|n| n.5).cloned();
                if let Some(attached) = attached {
                    let Some(owner) = attached.emitter_owner else { continue };
                    if let Some(p) = particles.as_deref_mut() {
                        for d in crate::particles::instance_systems(p, owner) {
                            if d.name.eq_ignore_ascii_case(&node) || d.emitter.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&node)) {
                                crate::particles::set_script_value(d, id, v);
                            }
                        }
                    }
                    continue;
                }
                if let Some(p) = particles.as_deref_mut() {
                    for d in crate::particles::placed_systems(p, r.0) {
                        if d.name.eq_ignore_ascii_case(&node) || d.emitter.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&node)) {
                            crate::particles::set_script_value(d, id, v);
                        }
                    }
                }
            }
            // The mirror has the value already; the shape is drawn with its own copy of
            // the material from now on [hypothesis: the original's change is this
            // reference's; the page's note puts an inventory reference's on the base].
            SceneEdit::Material { node, id, v, .. } => {
                // The player's base MODL is not an owned first-person shape source.
                // Refuse this unqualified path instead of borrowing third-person data.
                let attached = entity.and_then(|e| nodes.get(e).ok()).and_then(|n| n.5).cloned();
                let vals = if let Some(attached) = attached {
                    let Some(initial) = attached.material else { continue };
                    let values = attachment_materials.entry(entity.expect("attached entity")).or_insert(initial);
                    values[id] = v;
                    *values
                } else {
                    if view == ModelView::FirstPerson { continue; }
                    let Some(d) = data.as_deref() else { continue };
                    let model = fnv_game::nif_blocks::model_of(game, r).unwrap_or_default();
                    let table = tables.entry(model.clone()).or_insert_with(|| material_table(d, &model));
                    let base = node.split(':').next().unwrap_or(&node).to_string();
                    let Some(&start) = table.get(&node).or_else(|| table.get(&base)) else { continue };
                    let vals = materials.entry((r.0, view, node.clone())).or_insert(start);
                    vals[id] = v;
                    *vals
                };
                let Some(e) = entity else { continue };
                mutate_shape_material(e, &drawn, assets.as_deref_mut(), owned, &mut made_now, &mut commands, |m| {
                    set_material(m, &vals, id);
                });
            }
            // Live NiAlphaProperty override (JohnnyGuitar's SetAlphaPropertyValue):
            // updates the drawn shape's private NifMaterial `alpha_mode`.
            SceneEdit::AlphaProperty { node, item, v, .. } => {
                if item >= 2 { continue; }
                let attached = entity.and_then(|e| nodes.get(e).ok()).and_then(|n| n.5).cloned();
                let vals = if let Some(attached) = attached {
                    let Some(initial) = attached.source.alpha else { continue };
                    let values = attachment_alpha.entry(entity.expect("attached entity")).or_insert(initial);
                    values[item] = v;
                    *values
                } else {
                    if view == ModelView::FirstPerson { continue; }
                    let Some(d) = data.as_deref() else { continue };
                    let model = fnv_game::nif_blocks::model_of(game, r).unwrap_or_default();
                    let table = props.entry(model.clone()).or_insert_with(|| property_table(d, &model));
                    let base = node.split(':').next().unwrap_or(&node);
                    let Some(start) = table.get(&node).or_else(|| table.get(base)).and_then(|p| p.0) else { continue };
                    let vals = alpha_props.entry((r.0, view, node.clone())).or_insert(start);
                    vals[item] = v;
                    *vals
                };
                let Some(e) = entity else { continue };
                mutate_shape_material(e, &drawn, assets.as_deref_mut(), owned, &mut made_now, &mut commands, |m| {
                    set_alpha_property(m, &vals);
                });
            }
            // Live NiStencilProperty override (JohnnyGuitar's SetStencilPropertyValue):
            // updates the drawn shape's private NifMaterial `double_sided` / `cull_mode`
            // when flags (item 0) change; reference/mask (items 1-2) stay mirrored.
            SceneEdit::StencilProperty { node, item, v, .. } => {
                if item >= 3 { continue; }
                let attached = entity.and_then(|e| nodes.get(e).ok()).and_then(|n| n.5).cloned();
                let vals = if let Some(attached) = attached {
                    let Some(initial) = attached.source.stencil else { continue };
                    let values = attachment_stencil.entry(entity.expect("attached entity")).or_insert(initial);
                    values[item] = v;
                    *values
                } else {
                    if view == ModelView::FirstPerson { continue; }
                    let Some(d) = data.as_deref() else { continue };
                    let model = fnv_game::nif_blocks::model_of(game, r).unwrap_or_default();
                    let table = props.entry(model.clone()).or_insert_with(|| property_table(d, &model));
                    let base = node.split(':').next().unwrap_or(&node);
                    let Some(start) = table.get(&node).or_else(|| table.get(base)).and_then(|p| p.1) else { continue };
                    let vals = stencil_props.entry((r.0, view, node.clone())).or_insert(start);
                    vals[item] = v;
                    *vals
                };
                if item == 0 {
                    if let Some(e) = entity {
                        mutate_shape_material(e, &drawn, assets.as_deref_mut(), owned, &mut made_now, &mut commands, |m| {
                            set_stencil_property(m, &vals);
                        });
                    }
                }
            }
        }
    }
    // The held transforms, over this frame's pose.
    let keys: Vec<(u32, ModelView, String)> = held.keys().cloned().collect();
    for (r, view, node) in keys {
        let Some(root) = index.root_of(r, view) else { continue };
        let rig = index.rig_names(r, view);
        let h = held[&(r, view, node.clone())];
        apply_held(root, &node, h, &mut nodes, &rig);
    }
    // Name-based startup holds acquire a model as soon as it exists. They may
    // wait for a late child only in that model, then become concrete node holds.
    pending_held.retain(|&(r, view, ref name), pending| {
        let Some(root) = index.root_of(r, view) else { return pending.model.is_none() };
        if nodes.get(root).is_err() { return pending.model.is_none() }
        let model = root.to_bits();
        if pending.model.is_some_and(|captured| captured != model) { return false }
        pending.model = Some(model);
        let rig = index.rig_names(r, view);
        let Some(entity) = find(root, name, &nodes, &rig) else { return true };
        let identity = SceneIdentity { model, node: entity.to_bits() };
        if bound_entity(root, identity, &nodes, &rig).is_none() { return true }
        let value = bound_held.entry((r, view, identity)).or_default();
        merge_held(value, pending.value);
        false
    });
    // An edit holds over animation only while this exact model/node lives. A
    // disappearance, immediate root replacement, child ABA or subtree departure expires it.
    bound_held.retain(|&(r, view, identity), value| {
        let Some(root) = index.root_of(r, view) else { return false };
        let rig = index.rig_names(r, view);
        let Some(entity) = bound_entity(root, identity, &nodes, &rig) else { return false };
        apply_value(entity, value.value, &mut nodes);
        true
    });
    attachment_materials.retain(|e, _| {
        let alive = nodes.get(*e).is_ok_and(|n| n.5.is_some());
        if !alive { owned.remove(e); }
        alive
    });
    attachment_alpha.retain(|e, _| {
        let alive = nodes.get(*e).is_ok_and(|n| n.5.is_some());
        if !alive { owned.remove(e); }
        alive
    });
    attachment_stencil.retain(|e, _| {
        let alive = nodes.get(*e).is_ok_and(|n| n.5.is_some());
        if !alive { owned.remove(e); }
        alive
    });
    // The trees of the references scripts asked about.
    // Once scripts use the NIF functions (a reference was asked about; the set never
    // shrinks), the player's tree and those of references scripts created (actors placed
    // with PlaceAtMe, dropped items) are told every frame, watched or not: the original
    // has their 3D at once, so a script's first read finds it [observed: the nif-blocks
    // set's first reads of the player, of Doc's copy and of the pistol, #2132]. Any other
    // reference is told from the frame after a script first asks about it. A game whose
    // scripts never use them walks no tree here.
    for r in watched {
        if let Some(model) = index.drawn_flat(r.0) {
            let Some(d) = data.as_deref() else { continue };
            let file = files.entry(model.to_string()).or_insert_with(|| file_tree(d, model));
            let table = tables.entry(model.to_string()).or_insert_with(|| material_table(d, model));
            let tree = file_scene(r, file, table, held, materials);
            if logged.insert(r.0 ^ 0x4000_0000) {
                info!("nif mirror: {:08X} told from its model file {model:?} (drawn flat), {} nodes, first {:?}", r.0, tree.len(), tree.iter().take(4).map(|n| n.name.as_str()).collect::<Vec<_>>());
            }
            game.scene.set_tree(r, tree);
            continue;
        }
        let views: &[ModelView] = if r == FormId::PLAYER_REF {
            &[ModelView::ThirdPerson, ModelView::FirstPerson]
        } else { &[ModelView::Reference] };
        for &view in views {
            let Some(root) = index.root_of(r.0, view) else {
                if logged.insert(r.0) {
                    info!("nif mirror: {:08X} watched, not drawn", r.0);
                }
                game.scene.forget_model(r, view);
                continue;
            };
            // A rig resource can outlive the root's drawable Transform query.
            // Observing that absence must not publish authority for a later node.
            if nodes.get(root).is_err() {
                game.scene.forget_model(r, view);
                continue;
            }
            let rig = index.rig_names(r.0, view);
            let mut seen: HashMap<String, u32> = HashMap::new();
            // Per-view cache keys do not establish per-view file ownership. Until
            // the first-person model's actual shape source is supplied, metadata
            // from MODL/placed systems of the reference is unavailable for that rig.
            let model = if view == ModelView::FirstPerson { None }
                else { fnv_game::nif_blocks::model_of(game, r) };
            if let (Some(d), Some(model)) = (data.as_deref(), model.as_deref()) {
                if !tables.contains_key(model) {
                    tables.insert(model.to_string(), material_table(d, model));
                }
                if !props.contains_key(model) {
                    props.insert(model.to_string(), property_table(d, model));
                }
            }
            let table = model.as_deref().and_then(|m| tables.get(m));
            let prop_table = model.as_deref().and_then(|m| props.get(m));
            let systems: Vec<(String, Option<String>, [f32; 21])> = if view == ModelView::FirstPerson { Vec::new() } else { particles
                .as_deref_mut()
                .map(|p| crate::particles::placed_systems(p, r.0).into_iter().map(|d| (d.name.clone(), d.emitter.object.clone(), crate::particles::script_values(d))).collect())
                .unwrap_or_default() };
            let walked = walk(root, &nodes, &rig);
            let mut attached_emitters: HashMap<Entity, Vec<EmitterSummary>> = HashMap::new();
            if let Some(p) = particles.as_deref_mut() {
                for &(e, _) in &walked {
                    if let Some(owner) = nodes.get(e).ok().and_then(|n| n.5).and_then(|a| a.emitter_owner) {
                        attached_emitters.entry(owner).or_insert_with(|| {
                            crate::particles::instance_systems(p, owner)
                                .into_iter()
                                .map(|d| (d.name.clone(), d.emitter.object.clone(), crate::particles::script_values(d)))
                                .collect()
                        });
                    }
                }
            }
            // A player rig's root is told in the reference's own frame (its position and
            // angles), as the world positions compose it back (`fnv_game::nif_blocks::
            // node_place`): its pose, not the entity's transform, which the first-person
            // lens holds at the eye and the third person in the world. Without it a
            // first-person node's world position was the player's feet [observed: B42
            // True Leaning's `B42TLCamB` at the feet, its sideways ray along the ground].
            let posed = rigs.1.as_deref().filter(|_| r == FormId::PLAYER_REF).and_then(|p| p.posed_root(view == ModelView::FirstPerson));
            let placed = sim.as_deref().and_then(|s| s.world.peek(r)).map(|s| (s.pos, s.rot));
            let root_told = posed.zip(placed).map(|(p, (rp, ra))| {
                let rr = Quat::from_euler(EulerRot::ZYX, ra[2], ra[1], ra[0]);
                Transform { translation: rr.inverse() * (p.translation - Vec3::from(rp)), rotation: rr.inverse() * p.rotation, scale: p.scale }
            });
            let tree: Vec<SceneNode> = walked
                .iter()
                .enumerate()
                .map(|(k, &(e, parent))| (k, e, parent))
                .filter_map(|(k, e, parent)| {
                    let base = name_of(e, &nodes, &rig);
                    let (_, t, _, hidden, _, attached) = nodes.get(e).ok()?;
                    let t = root_told.filter(|_| k == 0 && rig.root == Some(e)).unwrap_or(*t);
                    // A name seen before gets ":0", ":1" … [observed: "Object", "Object:0"].
                    let n = seen.entry(base.to_ascii_lowercase()).or_insert(0);
                    let shown = if *n == 0 { base.clone() } else { format!("{base}:{}", *n - 1) };
                    *n += 1;
                    // Told in the scripts' angles: the drawn rotation's inverse (`apply_value`).
                    let (z, y, x) = t.rotation.inverse().to_euler(EulerRot::ZYX);
                    let material = if let Some(attached) = attached { attachment_materials.get(&e).copied().or(attached.material) }
                        else if view == ModelView::FirstPerson { None }
                        else {
                            (!materials.is_empty())
                                .then(|| materials.get(&(r.0, view, shown.clone())))
                                .flatten()
                                .or_else(|| table.and_then(|t| t.get(&base)))
                                .copied()
                        };
                    let (alpha, stencil) = if let Some(attached) = attached {
                        (
                            attachment_alpha.get(&e).copied().or(attached.source.alpha),
                            attachment_stencil.get(&e).copied().or(attached.source.stencil),
                        )
                    } else {
                        let (base_alpha, base_stencil) = prop_table.and_then(|p| p.get(&base)).copied().unwrap_or_default();
                        (
                            (!alpha_props.is_empty()).then(|| alpha_props.get(&(r.0, view, shown.clone())).copied()).flatten().or(base_alpha),
                            (!stencil_props.is_empty()).then(|| stencil_props.get(&(r.0, view, shown.clone())).copied()).flatten().or(base_stencil),
                        )
                    };
                    let emitter = if let Some(attached) = attached {
                        attached.emitter_owner
                            .and_then(|owner| attached_emitters.get(&owner))
                            .and_then(|list| list.iter().find(|(n, o, _)| n.eq_ignore_ascii_case(&base) || o.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&base))).map(|s| s.2))
                    } else {
                        systems.iter().find(|(n, o, _)| n.eq_ignore_ascii_case(&base) || o.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&base))).map(|s| s.2)
                    };
                    let info = NodeInfo { identity: Some(SceneIdentity { model: root.to_bits(), node: e.to_bits() }), bound: node_bound(k, &walked, &globals, &boxes), emitter, alpha, stencil };
                    Some(SceneNode {
                        name: shown,
                        parent,
                        translation: t.translation.to_array(),
                        rotation: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                        scale: t.scale.x,
                        flags: attached.map_or(0, |n| n.source.flags & !1) | u32::from(hidden),
                        material,
                        info: Some(Box::new(info)),
                    })
                })
                .collect();
            // A model drawn without its nodes' names, or without node entities (a cell model's,
            // TestTraps' pitching machine, #3128), is told from its file instead, as
            // a flat-drawn item is, so scripts and FireWeapon find its nodes by name.
            let unnamed = tree.iter().all(|n| n.name.is_empty() || n.name.starts_with(':'));
            if let (true, ModelView::Reference, Some(d), Some(model)) = (unnamed, view, data.as_deref(), model.as_deref()) {
                let file = files.entry(model.to_string()).or_insert_with(|| file_tree(d, model));
                if !file.is_empty() {
                    let table = tables.entry(model.to_string()).or_insert_with(|| material_table(d, model));
                    let tree = file_scene(r, file, table, held, materials);
                    if logged.insert(r.0 ^ 0x2000_0000) {
                        info!("nif mirror: {:08X} drawn without node names: told from its model file {model:?}, {} nodes, first {:?}", r.0, tree.len(), tree.iter().take(4).map(|n| n.name.as_str()).collect::<Vec<_>>());
                    }
                    game.scene.set_tree(r, tree);
                    continue;
                }
            }
            if logged.insert(r.0 | 0x8000_0000) {
                info!("nif mirror: {:08X} told, {} nodes, first {:?}", r.0, tree.len(), tree.iter().take(4).map(|n| n.name.as_str()).collect::<Vec<_>>());
            }
            game.scene.set_identified_model_for(r, view, root.to_bits(), tree);
        }
    }
    // The vertices of the shapes scripts asked about (SUP's NifBlockGetVerticles), in the
    // shape's own space as its mesh holds them; none for a mesh kept on the GPU only
    // [hypothesis: a skinned shape's are its bind pose].
    for (r, node) in wanted {
        let view = concrete_view(r.0, ModelView::Reference);
        let verts = index
            .root_of(r.0, view)
            .and_then(|root| find(root, &node, &nodes, &index.rig_names(r.0, view)))
            .and_then(|e| shapes.get(e).ok())
            .and_then(|m| meshes.as_deref()?.get(&m.0))
            // `try_`: a mesh extracted to the render world gives none, where `attribute` panics (#2202).
            .and_then(|mesh| match mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok()?? {
                bevy::mesh::VertexAttributeValues::Float32x3(v) => Some(v.clone()),
                _ => None,
            });
        game.scene.set_vertices(r, &node, verts);
    }
}


#[cfg(test)]
#[path = "nif_mirror/player_models_tests.rs"]
mod player_models_tests;

#[cfg(test)]
#[path = "nif_attachments/tests.rs"]
mod attachment_tests;

#[cfg(test)]
#[path = "camera_overlay/tests.rs"]
mod camera_overlay_tests;

#[cfg(test)]
#[path = "nif_mirror/visual_index_tests.rs"]
mod visual_index_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// The light in the air draws solid cores of some shapes (solid_cores.rs, #3032), loose
    /// entities of their own: switching it on and off leaves the shapes' components (their
    /// archetype) and children, the reference's root, the node walk and a dropped item's flat
    /// look as they were.
    #[test]
    fn the_lights_solid_cores_leave_the_model_as_it_is() {
        use crate::solid_cores::tests as cores;
        const R: u32 = 7;
        let mut app = cores::app(Some(true));
        let m = cores::add(&mut app, cores::tank());
        let plain = cores::add(&mut app, nvf_assets::nif_material(StandardMaterial::default()));
        // A reference drawn as two flat shapes of its cell, one with a core; one with nodes.
        let w = app.world_mut();
        let cell = w.spawn((Transform::IDENTITY, Visibility::Inherited)).id();
        let root = w.spawn((nvf_play::RefVisual(8), Transform::IDENTITY, Visibility::Inherited)).id();
        let item = w.spawn((nvf_play::dropped::DroppedRoot(9), nvf_play::dropped::DroppedModel("meshes\\item.nif".into()), Transform::IDENTITY, Visibility::Inherited)).id();
        let shapes = [(&m, cell, R), (&plain, cell, R), (&m, root, 8), (&m, item, 9)].map(|(m, parent, r)| {
            let e = cores::place(&mut app, m);
            app.world_mut().entity_mut(e).insert((nvf_play::RefVisual(r), Name::new("Shape"), ChildOf(parent)));
            e
        });
        app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited, Name::new("Below"), ChildOf(shapes[2])));
        let mut told = |on: bool| {
            app.insert_resource(crate::medium_air_demand::AirDraws(on));
            app.update();
            assert_eq!(cores::cores(&mut app).len(), if on { 3 } else { 0 });
            let w = app.world_mut();
            let kept = [cell, root, item, shapes[0], shapes[1], shapes[2], shapes[3]].map(|e| (w.entity(e).archetype().id(), w.get::<Children>(e).map(|c| c.to_vec())));
            let seen = w
                .run_system_once(move |rigs: Rigs, visuals: Visuals, nodes: NodeQuery, dropped: Dropped| {
                    let tops = [R, 8, 9].map(|r| root_of(r, ModelView::Reference, &rigs, &visuals));
                    let drawn: Vec<Entity> = visuals.iter().map(|v| v.0).collect();
                    (tops, drawn, walk(root, &nodes, &RigNames::default()), drawn_flat(9, &dropped))
                })
                .unwrap();
            (kept, seen)
        };
        let off = told(false);
        assert_eq!(off.1.2.len(), 2, "the shape and the node below it");
        assert_eq!(off.1.3.as_deref(), Some("item.nif"));
        for on in [true, false, true] {
            assert!(told(on) == off, "light {on}");
        }
    }

    /// #2981: a script's alpha-only edit writes every value back, glossiness included; on an
    /// authored material the set's roughness and reflectance stay, the glossiness goes to the
    /// legacy look and applies when the look is restored. A legacy material takes it at once.
    #[test]
    fn a_material_edit_keeps_an_authored_roughness_and_restores_the_scripted_gloss() {
        use nvf_assets::authored::{restore, LegacyLook, AUTHORED_MARK};
        let look = LegacyLook { normal: None, specular: None, roughness: 0.6, reflectance: 2.0, metallic: 0.0, tangents: true, specular_tint: Color::WHITE, parallax: Default::default(), scripted_tint: None };
        let mut m = nvf_assets::nif_material(StandardMaterial { perceptual_roughness: 0.8, reflectance: 0.5, ..default() });
        m.extension.emissive_rule.w = AUTHORED_MARK;
        m.extension.legacy = Some(look.clone());
        // Specular, emissive, glossiness 50, alpha 0.5 (the edit), emissive mult.
        let v = [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 50.0, 0.5, 1.0];
        set_material(&mut m, &v, 7);
        assert_eq!((m.base.perceptual_roughness, m.base.reflectance, m.base.base_color.alpha()), (0.8, 0.5, 0.5));
        let scripted = nvf_assets::nif_spawn::gloss_roughness(50.0);
        let kept = m.extension.legacy.clone().unwrap();
        assert_eq!(kept.roughness, scripted);
        restore(&mut m, &kept);
        assert_eq!((m.base.perceptual_roughness, m.base.reflectance), (scripted, 2.0));
        // A legacy material: drawn and kept alike.
        let mut l = nvf_assets::nif_material(StandardMaterial::default());
        l.extension.legacy = Some(look);
        set_material(&mut l, &v, 7);
        assert_eq!((l.base.perceptual_roughness, l.extension.legacy.unwrap().roughness), (scripted, scripted));
        // No legacy look (no set possible): as before.
        let mut plain = nvf_assets::nif_material(StandardMaterial::default());
        set_material(&mut plain, &v, 7);
        assert_eq!(plain.base.perceptual_roughness, scripted);
    }

    /// #2981: Bevy draws `specular_tint x reflectance`, and the mirror writes the stored
    /// specular colour back on every edit. An alpha-only edit on an authored shape whose stored
    /// colour is black keeps the set's highlight (white tint) and leaves the black colour for
    /// the legacy look; a colour the script sets itself is drawn; a legacy shape takes the
    /// stored colour as before.
    #[test]
    fn an_alpha_edit_keeps_an_authored_highlight_whatever_the_stored_specular_colour() {
        use nvf_assets::authored::{restore, LegacyLook, AUTHORED_MARK};
        let look = LegacyLook { normal: None, specular: None, roughness: 0.6, reflectance: 2.0, metallic: 0.0, tangents: true, specular_tint: Color::WHITE, parallax: Default::default(), scripted_tint: None };
        let black = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 0.5, 1.0];
        let mut m = nvf_assets::nif_material(StandardMaterial { reflectance: 0.5, ..default() });
        m.extension.emissive_rule.w = AUTHORED_MARK;
        m.extension.legacy = Some(look.clone());
        set_material(&mut m, &black, 7);
        assert_eq!((m.base.specular_tint, m.base.reflectance), (Color::WHITE, 0.5), "the set's highlight stays");
        assert_eq!(m.base.base_color.alpha(), 0.5);
        let kept = m.extension.legacy.clone().unwrap();
        assert_eq!(kept.specular_tint, Color::linear_rgb(0.0, 0.0, 0.0));
        let mut restored = m.clone();
        restore(&mut restored, &kept);
        assert_eq!(restored.base.specular_tint, Color::linear_rgb(0.0, 0.0, 0.0), "the scripted colour applies once the set is off");
        // The script sets the colour itself: drawn at once, set or not.
        let red = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 0.5, 1.0];
        set_material(&mut m, &red, 0);
        assert_eq!(m.base.specular_tint, Color::linear_rgb(1.0, 0.0, 0.0));
        // A legacy shape, and one with no legacy look: the stored colour, as before.
        let mut l = nvf_assets::nif_material(StandardMaterial::default());
        l.extension.legacy = Some(look);
        set_material(&mut l, &black, 7);
        let mut plain = nvf_assets::nif_material(StandardMaterial::default());
        set_material(&mut plain, &black, 7);
        for x in [&l, &plain] {
            assert_eq!(x.base.specular_tint, Color::linear_rgb(0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn a_light_source_is_not_a_references_root() {
        const R: u32 = 0x301;
        fn root(world: &mut World) -> Option<Entity> {
            world.run_system_once(|rigs: Rigs, visuals: Visuals| root_of(R, ModelView::Reference, &rigs, &visuals)).unwrap()
        }
        // Without a parent, so that each entity passes the test on its parent and only the
        // filter on light sources can leave it out.
        let mut world = World::new();
        world.spawn((PointLight::default(), RefVisual(R)));
        world.spawn((SpotLight::default(), RefVisual(R)));
        assert_eq!(root(&mut world), None, "its light sources alone");
        let drawn = world.spawn((Transform::IDENTITY, RefVisual(R))).id();
        assert_eq!(root(&mut world), Some(drawn), "not a light source beside it");
    }

    /// #2701: the full-detail ring's draw-only copy of a reference (`nvf_play::DrawOnly`) is
    /// never what the scripts' NIF functions find: only the loaded cell's own entity.
    #[test]
    fn a_draw_only_entity_is_not_a_references_root() {
        const R: u32 = 0x302;
        fn root(world: &mut World) -> Option<Entity> {
            world.run_system_once(|rigs: Rigs, visuals: Visuals| root_of(R, ModelView::Reference, &rigs, &visuals)).unwrap()
        }
        let mut world = World::new();
        world.spawn((Transform::IDENTITY, RefVisual(R), nvf_play::DrawOnly));
        assert_eq!(root(&mut world), None, "the ring's copy alone");
        assert_eq!(world.run_system_once(|v: Visuals| v.iter().count()).unwrap(), 0);
        let drawn = world.spawn((Transform::IDENTITY, RefVisual(R))).id();
        assert_eq!(root(&mut world), Some(drawn), "the loaded cell's entity");
    }

    #[test]
    fn explicitly_selected_player_root_does_not_fall_back_to_another_rig() {
        let mut world = World::new();
        let third = world.spawn(Transform::IDENTITY).id();
        let first = world.spawn(Transform::IDENTITY).id();
        assert_eq!(selected_player_rig(ModelView::ThirdPerson, || Some(third), || Some(first)), Some(third));
        assert_eq!(selected_player_rig(ModelView::FirstPerson, || Some(third), || Some(first)), Some(first));
        assert_eq!(selected_player_rig(ModelView::FirstPerson, || Some(third), || None::<Entity>), None);
        assert_eq!(selected_player_rig(ModelView::ThirdPerson, || None::<Entity>, || Some(first)), None);
    }

    #[test]
    fn legacy_default_is_third_person_and_explicit_selection_stays_concrete() {
        assert_eq!(concrete_view(FormId::PLAYER_REF.0, ModelView::Reference), ModelView::ThirdPerson);
        assert_eq!(concrete_view(FormId::PLAYER_REF.0, ModelView::FirstPerson), ModelView::FirstPerson);
        assert_eq!(concrete_view(0x301, ModelView::FirstPerson), ModelView::Reference);
    }

    #[test]
    fn tagged_held_edits_use_each_rigs_names_and_transform_entities() {
        let mut world = World::new();
        let third = world.spawn(Transform::IDENTITY).id();
        let first = world.spawn(Transform::IDENTITY).id();
        // Skeleton entities have no Name. Both rigs reuse the same authored bone
        // name; choosing the right rig's skeleton map matters as much as its root.
        let third_bone = world.spawn((Transform::from_xyz(30.0, 0.0, 0.0), ChildOf(third))).id();
        let first_bone = world.spawn((Transform::from_xyz(10.0, 0.0, 0.0), ChildOf(first))).id();
        let third_names = RigNames { root: Some(third), names: [(third, "ThirdRoot".into()), (third_bone, "SharedBone".into())].into() };
        let first_names = RigNames { root: Some(first), names: [(first, "FirstRoot".into()), (first_bone, "SharedBone".into())].into() };
        let mut held = HeldModels::new();
        for (view, edit) in [
            (ModelView::FirstPerson, SceneEdit::Translation { r: FormId::PLAYER_REF, node: "SharedBone".into(), v: [1.0, 2.0, 3.0] }),
            (ModelView::FirstPerson, SceneEdit::Scale { r: FormId::PLAYER_REF, node: "SharedBone".into(), v: 2.0 }),
            (ModelView::ThirdPerson, SceneEdit::Scale { r: FormId::PLAYER_REF, node: "SharedBone".into(), v: 3.0 }),
        ] {
            let (view, edit) = SceneEdit::ForModel { view, edit: Box::new(edit) }.into_model();
            assert!(remember_transform(view, &edit, &mut held));
        }
        assert_eq!(held.len(), 2, "one held entry per concrete player model");
        world.run_system_once(move |mut nodes: NodeQuery| {
            for view in [ModelView::ThirdPerson, ModelView::FirstPerson] {
                let (root, names) = selected_player_rig(view, || Some((third, &third_names)), || Some((first, &first_names))).unwrap();
                assert_eq!(find(root, "SharedBone", &nodes, names), Some(if view == ModelView::FirstPerson { first_bone } else { third_bone }));
                apply_held(root, "SharedBone", held[&(FormId::PLAYER_REF.0, view, "SharedBone".into())], &mut nodes, names);
            }
        }).unwrap();
        assert_eq!(world.get::<Transform>(first_bone).unwrap().translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(world.get::<Transform>(third_bone).unwrap().translation, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(world.get::<Transform>(first_bone).unwrap().scale, Vec3::splat(2.0));
        assert_eq!(world.get::<Transform>(third_bone).unwrap().scale, Vec3::splat(3.0));
    }

    /// #3105: a script material edit (`mutate_shape_material`) made while a presence fade
    /// (or overlapping `SetActorAlpha`) runs targets the mesh's own persistent material,
    /// refreshes the see-through copy in place at the combined opacity, and survives after
    /// the fade ends.
    #[test]
    fn a_script_material_edit_during_a_fade_is_kept_after_the_fade_ends() {
        let mut world = World::new();
        world.init_resource::<Assets<NifMaterial>>();
        let mut mat = nvf_assets::nif_material(StandardMaterial::default());
        mat.extension.depth_write = true;
        let own = world.resource_mut::<Assets<NifMaterial>>().add(mat);
        let e = world.spawn(MeshMaterial3d(own.clone())).id();
        // Start both presence fade (0.5) and actor alpha (0.5) -> combined factor 0.25.
        nvf_play::fade::apply_factor(&mut world, e, nvf_play::fade::FadeChannel::Presence, Some(0.5));
        nvf_play::fade::apply_factor(&mut world, e, nvf_play::fade::FadeChannel::ActorAlpha, Some(0.5));
        let fade_copy = world.get::<nvf_play::fade::Fading>(e).unwrap().copy().clone();
        let mut owned = HashMap::new();
        let mut made_now = HashMap::new();
        // First script edit during the fade: emissive + alpha = 0.8 + glossiness.
        let vals = [0.2, 0.3, 0.4, 0.9, 0.1, 0.2, 20.0, 0.8, 1.0];
        let mut owned_out = std::mem::take(&mut owned);
        let mut made_out = std::mem::take(&mut made_now);
        world.run_system_once(move |drawn: DrawnMaterials, mut assets: ResMut<Assets<NifMaterial>>, mut commands: Commands| {
            mutate_shape_material(e, &drawn, Some(&mut assets), &mut owned_out, &mut made_out, &mut commands, |m| {
                set_material(m, &vals, 3);
            });
            // A second edit in the same frame updates the same private persistent copy.
            let vals2 = [0.2, 0.3, 0.4, 0.9, 0.5, 0.2, 20.0, 0.8, 1.0];
            mutate_shape_material(e, &drawn, Some(&mut assets), &mut owned_out, &mut made_out, &mut commands, |m| {
                set_material(m, &vals2, 4);
            });
            assert_eq!(owned_out.len(), 1);
        }).unwrap();
        let edited = world.get::<nvf_play::fade::Fading>(e).expect("still fading").original().clone();
        assert_ne!(edited, own, "edit created a private persistent material");
        // While fading: the see-through copy shows the edited emissive and 0.8 * 0.25 alpha.
        {
            let mats = world.resource::<Assets<NifMaterial>>();
            let drawn = mats.get(&fade_copy).unwrap();
            assert_eq!(drawn.base.emissive, LinearRgba::rgb(0.9, 0.5, 0.2));
            assert!((drawn.base.base_color.alpha() - 0.8 * 0.25).abs() < 1e-6);
            // The persistent material itself keeps full edited alpha (0.8) and opaque mode.
            let base = mats.get(&edited).unwrap();
            assert_eq!(base.base.emissive, LinearRgba::rgb(0.9, 0.5, 0.2));
            assert!((base.base.base_color.alpha() - 0.8).abs() < 1e-6);
            assert_eq!(base.base.alpha_mode, AlphaMode::Opaque);
            assert!(base.extension.depth_write);
        }
        // End both fades: the edited material is put back, not the pre-edit original.
        nvf_play::fade::apply_factor(&mut world, e, nvf_play::fade::FadeChannel::Presence, None);
        nvf_play::fade::apply_factor(&mut world, e, nvf_play::fade::FadeChannel::ActorAlpha, None);
        assert!(world.get::<nvf_play::fade::Fading>(e).is_none());
        assert_eq!(world.get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, edited);
        let final_mat = world.resource::<Assets<NifMaterial>>().get(&edited).unwrap();
        assert_eq!(final_mat.base.emissive, LinearRgba::rgb(0.9, 0.5, 0.2));
        assert!((final_mat.base.base_color.alpha() - 0.8).abs() < 1e-6);
        assert_eq!(final_mat.base.alpha_mode, AlphaMode::Opaque);
    }

    #[test]
    fn mirror_index_matches_root_of_and_drawn_flat_across_hierarchies_and_drops() {
        const PARENT_CHILD_REF: u32 = 0x401;
        const CHILD_UNDER_OTHER_REF: u32 = 0x402;
        const LIGHT_ONLY_REF: u32 = 0x403;
        const DRAW_ONLY_REF: u32 = 0x404;
        const FLAT_DROP_REF: u32 = 0xFF00_0010;
        const NESTED_DROP_REF: u32 = 0xFF00_0011;
        const MISSING_REF: u32 = 0x4200_1234;

        let mut world = World::new();
        // Hierarchy where parent and child share the same RefVisual: parent wins.
        let top = world.spawn((Transform::IDENTITY, RefVisual(PARENT_CHILD_REF))).id();
        world.spawn((Transform::IDENTITY, RefVisual(PARENT_CHILD_REF), ChildOf(top)));
        // RefVisual entity parented under a different RefVisual: still its own root.
        let nested_other = world.spawn((Transform::IDENTITY, RefVisual(CHILD_UNDER_OTHER_REF), ChildOf(top))).id();
        // Light-only and DrawOnly entities are excluded.
        world.spawn((PointLight::default(), RefVisual(LIGHT_ONLY_REF)));
        world.spawn((Transform::IDENTITY, RefVisual(DRAW_ONLY_REF), nvf_play::DrawOnly));
        // Flat dropped item (children have no children of their own).
        let flat_root = world.spawn((
            nvf_play::dropped::DroppedRoot(FLAT_DROP_REF),
            nvf_play::dropped::DroppedModel("Meshes\\weapons\\pistol.nif".into()),
        )).id();
        world.spawn((Transform::IDENTITY, ChildOf(flat_root)));
        // Nested dropped item (child has a grandchild, so not drawn flat).
        let nested_root = world.spawn((
            nvf_play::dropped::DroppedRoot(NESTED_DROP_REF),
            nvf_play::dropped::DroppedModel("meshes\\clutter\\box.nif".into()),
        )).id();
        let nested_mid = world.spawn((Transform::IDENTITY, ChildOf(nested_root))).id();
        world.spawn((Transform::IDENTITY, ChildOf(nested_mid)));

        let all_refs = [
            PARENT_CHILD_REF,
            CHILD_UNDER_OTHER_REF,
            LIGHT_ONLY_REF,
            DRAW_ONLY_REF,
            FLAT_DROP_REF,
            NESTED_DROP_REF,
            MISSING_REF,
            FormId::PLAYER_REF.0,
        ];
        world.run_system_once(move |rigs: Rigs, visuals: Visuals, dropped: Dropped| {
            let wanted: HashSet<u32> = all_refs.into_iter().collect();
            let index = MirrorIndex::build(&wanted, &rigs, &visuals, &dropped);
            for r in all_refs {
                for view in [ModelView::Reference, ModelView::ThirdPerson, ModelView::FirstPerson] {
                    assert_eq!(
                        index.root_of(r, view),
                        root_of(r, view, &rigs, &visuals),
                        "root mismatch for 0x{r:08X} ({view:?})",
                    );
                }
                assert_eq!(
                    index.drawn_flat(r).map(str::to_string),
                    drawn_flat(r, &dropped),
                    "drawn_flat mismatch for 0x{r:08X}",
                );
            }
            assert_eq!(index.root_of(PARENT_CHILD_REF, ModelView::Reference), Some(top));
            assert_eq!(index.root_of(CHILD_UNDER_OTHER_REF, ModelView::Reference), Some(nested_other));
            assert_eq!(index.root_of(LIGHT_ONLY_REF, ModelView::Reference), None);
            assert_eq!(index.root_of(DRAW_ONLY_REF, ModelView::Reference), None);
            assert_eq!(index.drawn_flat(FLAT_DROP_REF), Some("weapons\\pistol.nif"));
            assert_eq!(index.drawn_flat(NESTED_DROP_REF), None);
            assert_eq!(index.root_of(MISSING_REF, ModelView::Reference), None);
        }).unwrap();
    }

    #[test]
    fn mirror_index_handles_thousands_of_watched_undrawn_refs_and_scene_visuals() {
        let mut world = World::new();
        let mut expected_root = None;
        for i in 0..2_000u32 {
            let r = 0x0010_0000 + i;
            let parent = world.spawn((Transform::IDENTITY, RefVisual(r))).id();
            world.spawn((Transform::IDENTITY, RefVisual(r), ChildOf(parent)));
            if i == 1_500 {
                expected_root = Some((r, parent));
            }
        }
        let (drawn_ref, drawn_entity) = expected_root.unwrap();
        let mut wanted: HashSet<u32> = (0..1_600u32).map(|i| 0x4200_0000 + i).collect();
        wanted.insert(drawn_ref);

        world.run_system_once(move |rigs: Rigs, visuals: Visuals, dropped: Dropped| {
            let index = MirrorIndex::build(&wanted, &rigs, &visuals, &dropped);
            assert_eq!(index.root_of(drawn_ref, ModelView::Reference), Some(drawn_entity));
            assert_eq!(index.visual_roots.len(), 1, "unwatched scene visuals must not populate the index");
            for i in 0..1_600u32 {
                assert_eq!(index.root_of(0x4200_0000 + i, ModelView::Reference), None);
            }
        }).unwrap();
    }
}
