//! JIP's NIF block functions (N6 of docs/handoff/nvse-next-batch.md;
//! docs/design/nif-blocks.md) [documented: their pages; observed: the peer's nif-blocks
//! set in the original game]. The nodes are the engine's; the game keeps a mirror of the
//! node tree of every reference a script asked about, filled by the engine each frame
//! (`SceneMirror::set_tree`), and queues the scripts' changes for the engine
//! (`SceneMirror::take_edits`). A change shows in the mirror at once [observed: a set
//! reads back in the same frame]. No memory layout of the original is used: nodes are
//! known by name, as on the pages (extender-compat.md §5.8).

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{HashMap, HashSet};

/// A concrete player model, or the ordinary reference model. In a script query,
/// `Reference` keeps the backend's existing third-person default (spec note #2387).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ModelView {
    #[default]
    Reference,
    ThirdPerson,
    FirstPerson,
}

/// One node as the scripts see it.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNode {
    pub name: String,
    pub parent: Option<usize>,
    /// Local translation (game units), rotation (degrees, X Y Z extrinsic) and scale. The
    /// angles are those of the transpose of the rotation the node is drawn with
    /// ([`drawn`]) [observed: GetNifBlockRotation of the 10mm pistol's ProjectileNode,
    /// 90 0 90, is its file matrix's rows taken as columns, the transpose of the drawn
    /// one; hypothesis: set angles are drawn the same way, as B42 True Leaning's lean
    /// right steps right and turns its nodes -18° about Y, a roll right only so (its
    /// `ln_B42Leaning.txt`); observer-checks.md 250].
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
    /// The NiAVObject flags; bit 0 hides it.
    pub flags: u32,
    /// A shape's NiMaterialProperty values in Get/SetMaterialPropertyValue's order:
    /// specular R G B, emissive R G B, glossiness, alpha, emissive mult [documented: the
    /// pages' value IDs 0-8]; `None` for a node or a shape without one.
    pub material: Option<[f32; 9]>,
    /// What else the engine tells of the node for JohnnyGuitar's NIF functions
    /// (`crate::nvse_jg_nif`); `None` when it told nothing more.
    pub info: Option<Box<NodeInfo>>,
}

/// Opaque identities supplied by the drawing backend. These are neither native
/// addresses nor core-owned handles; a backend validates both generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneIdentity {
    pub model: u64,
    pub node: u64,
}

/// The publication available when an edit was queued. An unpublished model keeps
/// the original pending-name contract; a published model pins any late-node lookup.
/// A missing/disappeared model cannot regain authority through a future namesake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneTarget {
    Legacy,
    Missing,
    Model(u64),
    Node(SceneIdentity),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TargetedSceneEdit {
    pub edit: SceneEdit,
    pub target: SceneTarget,
}

/// Structural attachment registration, consumed separately from numeric node edits.
/// The target is captured when the script calls, before the drawing backend runs.
#[derive(Debug, Clone)]
pub struct AttachmentChange {
    pub reference: FormId,
    pub view: ModelView,
    pub target: String,
    pub model: String,
    pub mode: u8,
    pub captured: SceneTarget,
}

/// A node's drawn bound, particle system and render properties, as the engine tells them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeInfo {
    /// Actual drawn model/node lifetime, when published by an identity-aware host.
    /// This does not qualify any material, property or emitter metadata.
    pub identity: Option<SceneIdentity>,
    /// The bounding sphere of the node and what is under it, in the world: centre X Y Z
    /// and radius (GetNiBound).
    pub bound: Option<[f32; 4]>,
    /// A particle system's values by GetNiPSysModifierValue's IDs 0-19, then its spawn
    /// rate (20).
    pub emitter: Option<[f32; 21]>,
    /// A shape's NiAlphaProperty: flags, threshold.
    pub alpha: Option<[f32; 2]>,
    /// A shape's NiStencilProperty: flags, reference, mask.
    pub stencil: Option<[f32; 3]>,
}

/// A change a script made, for the engine to apply to the drawn nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneEdit {
    /// The concrete player model selected when the edit was made. Kept with the
    /// request so a camera switch before consumption cannot redirect it.
    ForModel { view: ModelView, edit: Box<SceneEdit> },
    Translation { r: FormId, node: String, v: [f32; 3] },
    Rotation { r: FormId, node: String, v: [f32; 3] },
    Scale { r: FormId, node: String, v: f32 },
    Flags { r: FormId, node: String, flags: u32 },
    /// The flags of the `child`th child of `node` (0 first, in the order the engine told
    /// them), for a switch whose children are unnamed or share names (#2017).
    ChildFlags { r: FormId, node: String, child: usize, flags: u32 },
    Rename { r: FormId, node: String, to: String },
    Remove { r: FormId, node: String },
    /// A new empty node under `target`, or (`as_parent`) between `target` and its parent.
    Insert { r: FormId, target: String, name: String, as_parent: bool },
    /// A model's nodes under `target` (`model` under meshes).
    Attach { r: FormId, target: String, model: String },
    /// A light (LIGH `light`) as a new node `name` under `target`, at `offset` in that
    /// node's space (JIP's AttachLight, `crate::nvse_unlisted`).
    Light { r: FormId, target: String, name: String, light: FormId, offset: [f32; 3] },
    /// One NiMaterialProperty value of a shape (`id` 0-8, as in [`SceneNode::material`]).
    Material { r: FormId, node: String, id: usize, v: f32 },
    /// One NiAlphaProperty value of a shape (`item`: 0 flags, 1 threshold;
    /// JohnnyGuitar's SetAlphaPropertyValue, `crate::nvse_jg_nif`).
    AlphaProperty { r: FormId, node: String, item: usize, v: f32 },
    /// One NiStencilProperty value of a shape (`item`: 0 flags, 1 reference, 2 mask;
    /// JohnnyGuitar's SetStencilPropertyValue, `crate::nvse_jg_nif`).
    StencilProperty { r: FormId, node: String, item: usize, v: f32 },
    /// A particle system's value by GetNiPSysModifierValue's IDs, 20 the spawn rate
    /// (`crate::nvse_jg_nif`).
    Particle { r: FormId, node: String, id: usize, v: f32 },
}

impl SceneEdit {
    /// Remove the queue's model tag; ordinary legacy edits use `Reference`.
    pub fn into_model(self) -> (ModelView, Self) {
        match self {
            Self::ForModel { view, edit } => (view, *edit),
            edit => (ModelView::Reference, edit),
        }
    }

    fn reference(&self) -> FormId {
        match self {
            Self::ForModel { edit, .. } => edit.reference(),
            Self::Translation { r, .. } | Self::Rotation { r, .. } | Self::Scale { r, .. }
            | Self::Flags { r, .. } | Self::ChildFlags { r, .. } | Self::Rename { r, .. }
            | Self::Remove { r, .. } | Self::Insert { r, .. } | Self::Attach { r, .. } | Self::Light { r, .. }
            | Self::Material { r, .. } | Self::AlphaProperty { r, .. } | Self::StencilProperty { r, .. }
            | Self::Particle { r, .. } => *r,
        }
    }
}

/// The mirror and the queued changes.
#[derive(Debug, Default)]
pub struct SceneMirror {
    trees: HashMap<(FormId, ModelView), Vec<SceneNode>>,
    player_view: ModelView,
    watched: HashSet<FormId>,
    edits: Vec<TargetedSceneEdit>,
    attachment_changes: Vec<AttachmentChange>,
    attachment_session: std::sync::Arc<()>,
    identified: HashMap<(FormId, ModelView), Option<u64>>,
    /// The shapes whose vertices a script asked for (SUP's NifBlockGetVerticles), by
    /// reference and lower-case name, and what the engine told of them.
    vert_wanted: HashSet<(FormId, String)>,
    verts: HashMap<(FormId, String), Vec<[f32; 3]>>,
    /// The alpha and stencil property values scripts set (JohnnyGuitar's
    /// Set…PropertyValue), laid over each tree the engine tells: reference, node, alpha
    /// (else stencil), item, value.
    prop_sets: Vec<(FormId, ModelView, String, bool, usize, f32)>,
    /// What became of each runtime node in each drawn model (reference, model, node name
    /// in lower case), with that model's identity then ([`SceneMirror::insert_node`]): the
    /// same model is not given a second copy while the host has not shown the first, nor
    /// one a script took out of it; a model drawn anew gets the node again, told by its
    /// new identity or, without one, by a tree that showed the node and then lacked it.
    inserted: HashMap<(FormId, ModelView, String), (Option<u64>, RuntimeNodeState)>,
}

/// A runtime node in one drawn model ([`SceneMirror::inserted`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeNodeState {
    /// Queued; no tree the host told has shown it yet.
    Waiting,
    /// A tree the host told showed it: missing after that, the model was drawn anew.
    Shown,
    /// A script removed or renamed it: missing on purpose while this model lives.
    Taken,
}

impl SceneMirror {
    /// Publish separate player models while keeping legacy `set_tree` hosts on their
    /// single reference tree.
    pub fn use_separate_player_models(&mut self) { self.player_view = ModelView::ThirdPerson; }

    /// Whether the host publishes separate first-person and third-person player rigs.
    pub fn uses_separate_player_models(&self) -> bool { self.player_view != ModelView::Reference }

    pub fn resolve_view(&self, r: FormId, view: ModelView) -> ModelView {
        if r != FormId::PLAYER_REF { ModelView::Reference }
        else if view == ModelView::Reference { self.player_view }
        else { view }
    }

    /// Session ownership is separate from a movable Game address or a FormId.
    pub fn attachment_session(&self) -> std::sync::Arc<()> { self.attachment_session.clone() }

    pub fn take_attachment_changes(&mut self) -> Vec<AttachmentChange> {
        std::mem::take(&mut self.attachment_changes)
    }

    fn attachment_change(&mut self, r: FormId, view: ModelView, target: &str, model: &str, mode: u8) {
        let captured = self.target_for(view, &SceneEdit::Attach { r, target: target.into(), model: model.into() });
        self.attachment_changes.push(AttachmentChange {
            reference: r, view: self.resolve_view(r, view), target: target.into(), model: model.into(), mode, captured,
        });
        self.watched.insert(r);
    }

    /// The references whose trees the engine should tell.
    pub fn watched(&self) -> impl Iterator<Item = FormId> + '_ {
        self.watched.iter().copied()
    }

    /// Whether `r` is currently watched by scripts (O(1) fast-path check).
    pub fn is_watched(&self, r: FormId) -> bool {
        self.watched.contains(&r)
    }

    /// Have the engine tell `r`'s trees from now on, before any script asks (the player:
    /// a new game's first script question would otherwise find no tree and read 0).
    pub fn watch(&mut self, r: FormId) {
        self.watched.insert(r);
    }

    /// Whether the engine has told `r`'s tree for `view` (the player's first-person one
    /// with separate player models).
    pub fn has_tree(&self, r: FormId, view: ModelView) -> bool {
        self.trees.contains_key(&(r, self.resolve_view(r, view)))
    }

    /// The engine tells a watched reference's tree (parents before children).
    pub fn set_tree(&mut self, r: FormId, nodes: Vec<SceneNode>) {
        self.set_tree_for(r, ModelView::Reference, nodes);
    }

    /// Publish one model without replacing the other player rig's tree.
    pub fn set_tree_for(&mut self, r: FormId, view: ModelView, mut nodes: Vec<SceneNode>) {
        let view = self.resolve_view(r, view);
        // An explicit legacy publication deliberately opts this host back out.
        self.identified.remove(&(r, view));
        for ((ir, iv, name), (_, state)) in self.inserted.iter_mut() {
            if *ir == r && *iv == view && *state == RuntimeNodeState::Waiting && find(&nodes, name).is_some() {
                *state = RuntimeNodeState::Shown;
            }
        }
        for (pr, pv, node, alpha, item, v) in &self.prop_sets {
            if *pr == r && *pv == view {
                if let Some(i) = find(&nodes, node) {
                    lay_prop(&mut nodes[i], *alpha, *item, *v);
                }
            }
        }
        self.trees.insert((r, view), nodes);
    }

    /// An entity-backed host opts this model into concrete target capture. Even an
    /// empty tree is identified. A missing name can wait only within its published
    /// model identity; a disappeared model cannot become an unpublished target.
    pub fn set_identified_tree_for(&mut self, r: FormId, view: ModelView, nodes: Vec<SceneNode>) {
        let view = self.resolve_view(r, view);
        let model = nodes.iter().find_map(|node| node.info.as_ref()?.identity).map(|identity| identity.model);
        self.set_tree_for(r, view, nodes);
        self.identified.insert((r, view), model);
    }

    /// Publish an actual model root even when its script-visible node tree is empty.
    /// This captures no placeholder node and does not make cold getters succeed.
    pub fn set_identified_model_for(&mut self, r: FormId, view: ModelView, model: u64, nodes: Vec<SceneNode>) {
        let view = self.resolve_view(r, view);
        self.set_tree_for(r, view, nodes);
        self.identified.insert((r, view), Some(model));
    }

    /// A shape's alpha (`alpha`) or stencil property value for the session's reads and
    /// for the engine to draw ([`SceneEdit::AlphaProperty`] / [`SceneEdit::StencilProperty`]),
    /// kept over the trees the engine tells next; false without the node or that
    /// property on it.
    pub fn set_prop(&mut self, r: FormId, node: &str, alpha: bool, item: usize, v: f32) -> bool {
        let view = self.resolve_view(r, ModelView::Reference);
        let Some(t) = self.tree(r) else { return false };
        let Some(i) = find(t, node) else { return false };
        if !lay_prop(&mut t[i], alpha, item, v) {
            return false;
        }
        let name = t[i].name.clone();
        self.prop_sets.retain(|(pr, pv, n, a, k, _)| !(*pr == r && *pv == view && n.eq_ignore_ascii_case(&name) && *a == alpha && *k == item));
        self.prop_sets.push((r, view, name.clone(), alpha, item, v));
        let edit = if alpha {
            SceneEdit::AlphaProperty { r, node: name, item, v }
        } else {
            SceneEdit::StencilProperty { r, node: name, item, v }
        };
        self.queue_for(ModelView::Reference, edit);
        true
    }

    /// The reference's 3D is gone.
    pub fn forget(&mut self, r: FormId) {
        self.trees.retain(|(reference, _), _| *reference != r);
        // What scripts took out was out of that 3D only (ext-jippp.md §1).
        self.inserted.retain(|(ir, _, _), (_, state)| *ir != r || *state != RuntimeNodeState::Taken);
    }

    pub fn forget_model(&mut self, r: FormId, view: ModelView) {
        let view = self.resolve_view(r, view);
        self.trees.remove(&(r, view));
        self.inserted.retain(|(ir, iv, _), (_, state)| *ir != r || *iv != view || *state != RuntimeNodeState::Taken);
    }

    /// A script took these names out of one drawn model (RemoveNifBlock, with what was
    /// under the block; RenameNifBlock): the runtime node list leaves them out of it.
    fn taken_out(&mut self, r: FormId, view: ModelView, names: Vec<String>) {
        let view = self.resolve_view(r, view);
        let model = self.identified.get(&(r, view)).copied().flatten();
        for name in names {
            self.inserted.insert((r, view, name.to_ascii_lowercase()), (model, RuntimeNodeState::Taken));
        }
    }

    /// A new InsertNode call asks again for what a script took out.
    fn asked_again(&mut self, r: FormId, name: &str) {
        self.inserted.retain(|(ir, _, n), (_, state)| *ir != r || *state != RuntimeNodeState::Taken || !n.eq_ignore_ascii_case(name));
    }

    /// The changes made since the last call, for the engine.
    pub fn take_edits(&mut self) -> Vec<SceneEdit> {
        self.take_targeted_edits().into_iter().map(|queued| queued.edit).collect()
    }

    /// The real drawing backend keeps the identities captured when scripts queued
    /// their changes. Legacy `take_edits` deliberately exposes only the old payload.
    pub fn take_targeted_edits(&mut self) -> Vec<TargetedSceneEdit> {
        // The real attachment producer drains its side queue before this mirror.
        // Legacy/headless hosts consuming edits must not retain an unconsumed copy.
        self.attachment_changes.clear();
        std::mem::take(&mut self.edits)
    }

    fn tree(&mut self, r: FormId) -> Option<&mut Vec<SceneNode>> {
        self.tree_for(r, ModelView::Reference)
    }

    fn tree_for(&mut self, r: FormId, view: ModelView) -> Option<&mut Vec<SceneNode>> {
        self.watched.insert(r);
        let view = self.resolve_view(r, view);
        self.trees.get_mut(&(r, view))
    }

    fn target_for(&self, view: ModelView, edit: &SceneEdit) -> SceneTarget {
        let r = edit.reference();
        let view = self.resolve_view(r, view);
        let Some(model) = self.identified.get(&(r, view)) else { return SceneTarget::Legacy };
        let Some(tree) = self.trees.get(&(r, view)) else { return SceneTarget::Missing };
        let name = match edit {
            SceneEdit::Rename { node, .. } => node,
            SceneEdit::Insert { target, .. } | SceneEdit::Attach { target, .. } | SceneEdit::Light { target, .. } => target,
            SceneEdit::Translation { node, .. } | SceneEdit::Rotation { node, .. }
            | SceneEdit::Scale { node, .. } | SceneEdit::Flags { node, .. }
            | SceneEdit::ChildFlags { node, .. } | SceneEdit::Remove { node, .. }
            | SceneEdit::Material { node, .. } | SceneEdit::AlphaProperty { node, .. }
            | SceneEdit::StencilProperty { node, .. } | SceneEdit::Particle { node, .. } => node,
            SceneEdit::ForModel { .. } => return SceneTarget::Missing,
        };
        let missing_node = model.map_or(SceneTarget::Missing, SceneTarget::Model);
        let Some(mut index) = find(tree, name) else { return missing_node };
        if let SceneEdit::ChildFlags { child, .. } = edit {
            let Some(found) = tree.iter().enumerate().filter(|(_, n)| n.parent == Some(index)).nth(*child).map(|(i, _)| i) else { return SceneTarget::Missing };
            index = found;
        }
        tree[index].info.as_ref().and_then(|info| info.identity)
            .map_or(missing_node, SceneTarget::Node)
    }

    fn queue_targeted(&mut self, view: ModelView, edit: SceneEdit, target: SceneTarget) {
        let view = self.resolve_view(edit.reference(), view);
        let edit = if view == ModelView::Reference { edit }
            else { SceneEdit::ForModel { view, edit: Box::new(edit) } };
        self.edits.push(TargetedSceneEdit { edit, target });
    }

    fn queue_for(&mut self, view: ModelView, edit: SceneEdit) {
        let target = self.target_for(view, &edit);
        self.queue_targeted(view, edit, target);
    }

    /// InsertNode's node `name` under `target` (`as_parent`: between `target` and its
    /// parent) in each drawn model of `r` that has `target` and lacks `name`, the player's
    /// two models both: shown in the mirror at once and queued for the engine. A model is
    /// given the node once ([`SceneMirror::inserted`]). Whether any drawn model has
    /// `target`.
    pub(crate) fn insert_node(&mut self, r: FormId, target: &str, name: &str, as_parent: bool) -> bool {
        self.watched.insert(r);
        let mut views: Vec<ModelView> = self.trees.keys().filter(|(t, _)| *t == r).map(|(_, v)| *v).collect();
        views.sort_by_key(|v| *v as u8);
        let mut any = false;
        for view in views {
            let model = self.identified.get(&(r, view)).copied().flatten();
            let key = (r, view, name.to_ascii_lowercase());
            let Some(t) = self.trees.get_mut(&(r, view)) else { continue };
            let Some(i) = find(t, target) else { continue };
            any = true;
            if find(t, name).is_some() || self.inserted.get(&key).is_some_and(|&(m, state)| m == model && state != RuntimeNodeState::Shown) {
                continue;
            }
            let node = SceneNode { name: name.to_string(), parent: if as_parent { t[i].parent } else { Some(i) }, translation: [0.0; 3], rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None };
            t.push(node);
            if as_parent {
                let new = t.len() - 1;
                t[i].parent = Some(new);
            }
            self.inserted.insert(key, (model, RuntimeNodeState::Waiting));
            self.queue_for(view, SceneEdit::Insert { r, target: target.to_string(), name: name.to_string(), as_parent });
        }
        any
    }

    /// A node of a reference's tree as the mirror holds it now (the reference watched
    /// from now on); `None` without the tree or the node.
    pub fn node_now(&mut self, r: FormId, node: &str) -> Option<SceneNode> {
        self.node_for_model(r, ModelView::Reference, node)
    }

    /// Inspect one selected mirror without borrowing names or metadata from another
    /// player model. This is the Rust-side mirror API, not a new script function.
    pub fn node_for_model(&mut self, r: FormId, view: ModelView, node: &str) -> Option<SceneNode> {
        let t = self.tree_for(r, view)?;
        find(t, node).map(|i| t[i].clone())
    }

    /// A particle system's value for the engine (`SceneEdit::Particle`), shown in the
    /// mirror at once; false without the node or a particle system there.
    pub fn set_particle(&mut self, r: FormId, node: &str, id: usize, v: f32) -> bool {
        let Some(t) = self.tree(r) else { return false };
        let Some(i) = find(t, node) else { return false };
        let Some(e) = t[i].info.as_mut().and_then(|n| n.emitter.as_mut()) else { return false };
        let Some(slot) = e.get_mut(id) else { return false };
        *slot = v;
        let node = t[i].name.clone();
        self.queue_for(ModelView::Reference, SceneEdit::Particle { r, node, id, v });
        true
    }

    /// A translation, rotation or scale for the engine, shown in the mirror at once as
    /// the NIF block setters' (SUP's interpolations, `crate::nvse_sup7`).
    pub fn push_edit(&mut self, e: SceneEdit) {
        let (view, e) = e.into_model();
        let (r, node) = match &e {
            SceneEdit::Translation { r, node, .. } | SceneEdit::Rotation { r, node, .. } | SceneEdit::Scale { r, node, .. } => (*r, node.clone()),
            _ => {
                self.queue_for(view, e);
                return;
            }
        };
        if let Some(t) = self.tree_for(r, view) {
            if let Some(i) = find(t, &node) {
                match &e {
                    SceneEdit::Translation { v, .. } => t[i].translation = *v,
                    SceneEdit::Rotation { v, .. } => t[i].rotation = *v,
                    SceneEdit::Scale { v, .. } => t[i].scale = *v,
                    _ => {}
                }
            }
        }
        self.queue_for(view, e);
    }

    /// A shape's vertices as the engine last told them, in the shape's own space; asked
    /// for from now on. `None` until told, or when the reference draws no such shape.
    pub fn vertices(&mut self, r: FormId, node: &str) -> Option<Vec<[f32; 3]>> {
        let key = (r, node.to_ascii_lowercase());
        let v = self.verts.get(&key).cloned();
        self.vert_wanted.insert(key);
        v
    }

    /// The shapes whose vertices the engine should tell (reference, lower-case name).
    pub fn wanted_vertices(&self) -> impl Iterator<Item = &(FormId, String)> + '_ {
        self.vert_wanted.iter()
    }

    /// The engine tells a shape's vertices (none: it draws no shape of that name).
    pub fn set_vertices(&mut self, r: FormId, node: &str, v: Option<Vec<[f32; 3]>>) {
        let key = (r, node.to_ascii_lowercase());
        match v {
            Some(v) => self.verts.insert(key, v),
            None => self.verts.remove(&key),
        };
    }
}

impl Game<'_> {
    /// A model's nodes attached at once under a node of a reference's drawn tree (its
    /// root when `target` is `None`), for the engine (JIPPPLN's AttachFormModel); false
    /// when the tree or the node is not known.
    pub(crate) fn attach_model_now(&mut self, r: FormId, target: Option<&str>, model: &str) -> bool {
        let Some(t) = self.scene.tree(r) else { return false };
        let node = match target {
            Some(n) => find(t, n).map(|i| t[i].name.clone()),
            None => t.iter().find(|n| n.parent.is_none()).map(|n| n.name.clone()),
        };
        let Some(node) = node else { return false };
        self.scene.queue_for(ModelView::Reference, SceneEdit::Attach { r, target: node, model: model.to_string() });
        true
    }

    /// A light under a node of a reference's drawn tree, shown in the mirror at once as a
    /// node of its own named `name` (JIP's AttachLight); false when the tree or the node
    /// is not known.
    pub(crate) fn attach_light_now(&mut self, r: FormId, target: &str, light: FormId, name: &str, offset: [f32; 3]) -> bool {
        let Some(t) = self.scene.tree(r) else { return false };
        let Some(i) = find(t, target) else { return false };
        let target = t[i].name.clone();
        t.push(SceneNode { name: name.to_string(), parent: Some(i), translation: offset, rotation: [0.0; 3], scale: 1.0, flags: 0, material: None, info: None });
        self.scene.queue_for(ModelView::Reference, SceneEdit::Light { r, target, name: name.to_string(), light, offset });
        true
    }

    /// A node of a reference's drawn tree taken away, for the engine (JIPPPLN's
    /// ClearRuntimeNodes); the mirror forgets it at once.
    pub(crate) fn remove_node_now(&mut self, r: FormId, node: &str) {
        crate::nif_blocks::call(self, "removenifblock", Some(r), &[XVal::Str(node.to_string())]);
    }
}

/// Puts an alpha or stencil property value on a node; false when it has no such property
/// or item.
fn lay_prop(n: &mut SceneNode, alpha: bool, item: usize, v: f32) -> bool {
    let Some(info) = n.info.as_deref_mut() else { return false };
    let slot = if alpha { info.alpha.as_mut().map(|a| a.as_mut_slice()) } else { info.stencil.as_mut().map(|s| s.as_mut_slice()) };
    match slot.and_then(|s| s.get_mut(item)) {
        Some(x) => {
            *x = v;
            true
        }
        None => false,
    }
}

/// The node of that name: exactly, else without regard to case [observed:
/// "bip01 head|…" still inserted; ModelHasBlock is exact].
fn find(t: &[SceneNode], name: &str) -> Option<usize> {
    t.iter().position(|n| n.name == name).or_else(|| t.iter().position(|n| n.name.eq_ignore_ascii_case(name)))
}

/// Every node under `i`, depth first in order.
fn descendants(t: &[SceneNode], i: Option<usize>) -> Vec<usize> {
    let mut out = Vec::new();
    fn walk(t: &[SceneNode], p: Option<usize>, out: &mut Vec<usize>) {
        for (k, n) in t.iter().enumerate() {
            if n.parent == p && Some(k) != p {
                out.push(k);
                walk(t, Some(k), out);
            }
        }
    }
    walk(t, i, &mut out);
    out
}

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

/// The axis an argument names (`X`, `Y`, `Z`), when it names one.
fn axis(v: Option<&XVal>) -> Option<usize> {
    match v? {
        XVal::Str(s) => match s.to_ascii_uppercase().as_str() {
            "X" => Some(0),
            "Y" => Some(1),
            "Z" => Some(2),
            _ => None,
        },
        _ => None,
    }
}

/// A result and values for the script variables the VM writes (`Vm::out_call`).
fn outputs(g: &mut Game, ret: f64, vals: &[f64]) -> XVal {
    let entries = std::iter::once(ret).chain(vals.iter().copied()).enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(v))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

pub(crate) type Mat = [[f32; 3]; 3];

pub(crate) fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut o = [[0.0; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, c) in row.iter_mut().enumerate() {
            *c = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

pub(crate) fn apply(m: &Mat, v: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

/// The rotation of X Y Z angles (radians) taken about the fixed axes in that order,
/// Rz·Ry·Rx: the mirror's convention (`SceneNode::rotation`, the engine's `EulerRot::ZYX`).
pub(crate) fn rotation(a: [f32; 3]) -> Mat {
    let (sx, cx) = a[0].sin_cos();
    let (sy, cy) = a[1].sin_cos();
    let (sz, cz) = a[2].sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mul(&rz, &mul(&ry, &rx))
}

/// The rotation a node of those angles (radians, [`SceneNode::rotation`]) is drawn with.
pub(crate) fn drawn(a: [f32; 3]) -> Mat {
    transpose(&rotation(a))
}

pub(crate) fn transpose(m: &Mat) -> Mat {
    [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]
}

/// [`rotation`]'s angles back, in degrees.
pub(crate) fn extrinsic_angles(m: &Mat) -> [f32; 3] {
    let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
    [m[2][1].atan2(m[2][2]), y, m[1][0].atan2(m[0][0])].map(f32::to_degrees)
}

/// A node's translation and drawn rotation ([`drawn`]): as its parent holds it, or
/// (`world`) in the world, through the nodes above it and the reference's own position
/// and angles. `None` without the tree or the node.
pub(crate) fn node_place(g: &mut Game, r: FormId, node: &str, world: bool) -> Option<([f32; 3], Mat)> {
    node_place_for(g, r, node, world, ModelView::Reference)
}

fn node_place_for(g: &mut Game, r: FormId, node: &str, world: bool, view: ModelView) -> Option<([f32; 3], Mat)> {
    let t = g.scene.tree_for(r, view)?.clone();
    let mut i = find(&t, node)?;
    let local = |n: &SceneNode| (n.translation, drawn(n.rotation.map(f32::to_radians)), n.scale);
    let (mut pos, mut rot, _) = local(&t[i]);
    if !world {
        return Some((pos, rot));
    }
    while let Some(p) = t[i].parent {
        let (pt, pr, ps) = local(&t[p]);
        pos = apply(&pr, pos.map(|x| x * ps));
        pos = [pos[0] + pt[0], pos[1] + pt[1], pos[2] + pt[2]];
        rot = mul(&pr, &rot);
        i = p;
    }
    let (rp, ra) = g.ref_pos_rot(r)?;
    let rr = rotation(ra);
    let p = apply(&rr, pos);
    Some(([p[0] + rp[0], p[1] + rp[1], p[2] + rp[2]], mul(&rr, &rot)))
}

/// A node's place in the world (the reference's own place, and the nodes above it), for
/// JohnnyGuitar's distances (`crate::nvse_jg_nif`); `None` without the tree or the node.
pub(crate) fn node_world(g: &mut Game, r: FormId, node: &str) -> Option<[f32; 3]> {
    node_place(g, r, node, true).map(|(p, _)| p)
}

/// Shows only the `show`th child of the node of that name (none for a place past the
/// children), each child addressed by its place under the node, not by its name; the
/// engine gets one [`SceneEdit::ChildFlags`] per child. `false` without the tree or the
/// node.
pub(crate) fn show_one_child(g: &mut Game, r: FormId, node: &str, show: Option<usize>) -> bool {
    let Some(t) = g.scene.tree(r) else { return false };
    let Some(i) = find(t, node) else { return false };
    let kids: Vec<usize> = (0..t.len()).filter(|&k| t[k].parent == Some(i) && k != i).collect();
    let mut edits = Vec::new();
    for (place, k) in kids.into_iter().enumerate() {
        let flags = if Some(place) == show { t[k].flags & !1 } else { t[k].flags | 1 };
        t[k].flags = flags;
        edits.push(SceneEdit::ChildFlags { r, node: node.to_string(), child: place, flags });
    }
    for edit in edits { g.scene.queue_for(ModelView::Reference, edit); }
    true
}

/// The mirror's node of that name and its children, for JohnnyGuitar's NIF functions.
pub(crate) fn node_and_children(g: &mut Game, r: FormId, node: &str) -> Option<(SceneNode, Vec<String>)> {
    let t = g.scene.tree(r)?;
    let i = find(t, node)?;
    let kids = t.iter().enumerate().filter(|(k, n)| n.parent == Some(i) && *k != i).map(|(_, n)| n.name.clone()).collect();
    Some((t[i].clone(), kids))
}

/// The name of the reference's root node in the mirror (the first without a parent).
pub(crate) fn root_name(g: &mut Game, r: FormId) -> Option<String> {
    g.scene.tree(r)?.iter().find(|n| n.parent.is_none()).map(|n| n.name.clone())
}

/// The model file of a base form (MODL), under meshes.
pub fn model_of(g: &Game, f: FormId) -> Option<String> {
    let b = crate::functions::base_of(g, f).unwrap_or(f);
    match crate::nvse::record_value(g, b, "MODL", "").or_else(|| crate::nvse::record_value(g, b, "MODL", "model")) {
        Some(XVal::Str(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let list = |g: &mut Game, items: Vec<XVal>| {
        let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
        XVal::Array(g.ext.new_array(ArrayKind::List, entries))
    };
    let r = subject.unwrap_or(FormId(0));
    // The VM removes Alt output variables before dispatch. Axis overloads have
    // one extra input; this is JIP's 1/2 convention, not JohnnyGuitar's 0/1.
    let selector = match name {
        "getchildblocks" | "getnifblockparentnodes" | "getnifblockscale" | "removenifblock" => 1,
        "getnifblocktranslation" | "getnifblockrotation" => if axis(args.get(1)).is_some() { 3 } else { 2 },
        "getnifblocktranslationalt" | "getnifblockrotationalt" | "getnifblockflag"
        | "setnifblockscale" | "renamenifblock" => 2,
        "setnifblocktranslation" => 4,
        "setnifblockrotation" => 5,
        "setnifblockflag" => 3,
        _ => usize::MAX,
    };
    let view = match num(args, selector, 0.0) {
        1.0 => ModelView::ThirdPerson,
        2.0 => ModelView::FirstPerson,
        // Omitted/0 `pcModel` on the player follows the active 1st/3rd-person camera
        // when separate player models are published [observed: Fallout: New Vegas
        // 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy
        // (read-only `PrintToConsole` probe; zero writes to game install or Saves),
        // `w4d_43_nif_selector_and_ui_entities`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, PR #3803 / #3802):
        // `PROBE|w4d_43_nif_selector_and_ui_entities|p1(1p=1,cb=-1/12/-1/-1)|p3(1p=0,cb=12/12/-1/-1)|ui(ret=64,f=0[&false],t=0,c=0)|idle(1,0,1)`;
        // zero DLL, PDB, or third-party source consulted — in 1st person (`1p=1`),
        // omitted `pcModel` returned `-1` (matching `pcModel=2` `-1` rather than
        // `pcModel=1` `12`), and in 3rd person (`1p=0`), omitted `pcModel` returned
        // `12` (matching `pcModel=1` `12`)]. Legacy single-tree hosts and non-`pcModel`
        // functions stay on `ModelView::Reference`.
        _ if selector != usize::MAX && r == FormId::PLAYER_REF && g.scene.uses_separate_player_models() => {
            if g.senses.first_person { ModelView::FirstPerson } else { ModelView::ThirdPerson }
        }
        _ => ModelView::Reference,
    };
    let v = match name {
        // A missing block, or 3D not there yet, gives ar_Null [observed].
        "getchildblocks" => {
            let parent = text(args, 0);
            let Some(t) = g.scene.tree_for(r, view).cloned() else { return Some(Ok(XVal::Array(0))) };
            let at = if parent.is_empty() { None } else {
                match find(&t, &parent) {
                    Some(i) => Some(i),
                    None => return Some(Ok(XVal::Array(0))),
                }
            };
            let names = descendants(&t, at).into_iter().map(|i| XVal::Str(t[i].name.clone())).collect();
            list(g, names)
        }
        // Innermost first [observed: Bip01 Neck1 … Scene Root].
        "getnifblockparentnodes" => {
            let Some(t) = g.scene.tree_for(r, view).cloned() else { return Some(Ok(XVal::Array(0))) };
            let Some(mut i) = find(&t, &text(args, 0)) else { return Some(Ok(XVal::Array(0))) };
            let mut out = Vec::new();
            while let Some(p) = t[i].parent {
                out.push(XVal::Str(t[p].name.clone()));
                i = p;
            }
            list(g, out)
        }
        // JIP: `block [getWorld] [pcModel]`, or ButcherPete's axis overload.
        // Compose through the existing Alt getter's world transform route.
        "getnifblocktranslation" => {
            let component = axis(args.get(1));
            let world = num(args,if component.is_some() {2} else {1},0.0) != 0.0;
            let Some((v,_)) = node_place_for(g,r,&text(args,0),world,view) else {return Some(Ok(XVal::Array(0)))};
            match component {
                Some(a) => XVal::Num(f64::from(v[a])),
                None => list(g,v.into_iter().map(|x|XVal::Num(f64::from(x))).collect()),
            }
        }
        // `block [axis]`: an axis gives that number; otherwise the three as an array
        // [observed: JIP 57.3 gives [5.59, 0, 0] for Bip01 Head].
        "getnifblockrotation" => {
            let Some(t) = g.scene.tree_for(r, view) else { return Some(Ok(XVal::Array(0))) };
            let Some(i) = find(t, &text(args, 0)) else { return Some(Ok(XVal::Array(0))) };
            let v = t[i].rotation;
            match axis(args.get(1)) {
                Some(a) => XVal::Num(f64::from(v[a])),
                None => {
                    let items = v.iter().map(|x| XVal::Num(f64::from(*x))).collect();
                    list(g, items)
                }
            }
        }
        "setnifblocktranslation" | "setnifblockrotation" => {
            let node = text(args, 0);
            let v = [num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32, num(args, 3, 0.0) as f32];
            if let Some(t) = g.scene.tree_for(r, view) {
                if let Some(i) = find(t, &node) {
                    if name == "setnifblocktranslation" { t[i].translation = v } else { t[i].rotation = v }
                }
            }
            g.scene.queue_for(view, if name == "setnifblocktranslation" { SceneEdit::Translation { r, node, v } } else { SceneEdit::Rotation { r, node, v } });
            zero
        }
        // A missing block 0 [observed].
        "getnifblockscale" => {
            let Some(t) = g.scene.tree_for(r, view) else { return Some(Ok(zero)) };
            XVal::Num(find(t, &text(args, 0)).map_or(0.0, |i| f64::from(t[i].scale)))
        }
        "setnifblockscale" => {
            let node = text(args, 0);
            let v = num(args, 1, 1.0) as f32;
            if let Some(t) = g.scene.tree_for(r, view) {
                if let Some(i) = find(t, &node) {
                    t[i].scale = v;
                }
            }
            g.scene.queue_for(view, SceneEdit::Scale { r, node, v });
            zero
        }
        "getnifblockflag" => {
            let Some(t) = g.scene.tree_for(r, view) else { return Some(Ok(zero)) };
            let bit = num(args, 1, 0.0) as u32;
            XVal::Num(find(t, &text(args, 0)).map_or(0.0, |i| f64::from((t[i].flags >> bit.min(31)) & 1)))
        }
        "setnifblockflag" => {
            let node = text(args, 0);
            let bit = 1u32 << (num(args, 1, 0.0) as u32).min(31);
            let on = num(args, 2, 0.0) != 0.0;
            let mut flags = None;
            if let Some(t) = g.scene.tree_for(r, view) {
                if let Some(i) = find(t, &node) {
                    t[i].flags = if on { t[i].flags | bit } else { t[i].flags & !bit };
                    flags = Some(t[i].flags);
                }
            }
            if let Some(flags) = flags {
                g.scene.queue_for(view, SceneEdit::Flags { r, node, flags });
            }
            zero
        }
        "renamenifblock" => {
            let (node, to) = (text(args, 0), text(args, 1));
            let target = g.scene.target_for(view, &SceneEdit::Rename { r, node: node.clone(), to: to.clone() });
            let old = g.scene.tree_for(r, view).and_then(|t| find(t, &node).map(|i| std::mem::replace(&mut t[i].name, to.clone())));
            let ok = old.is_some();
            if let Some(old) = old {
                g.scene.taken_out(r, view, vec![old]);
                g.scene.queue_targeted(view, SceneEdit::Rename { r, node, to }, target);
            }
            XVal::Num(f64::from(u8::from(ok)))
        }
        "removenifblock" => {
            let node = text(args, 0);
            let target = g.scene.target_for(view, &SceneEdit::Remove { r, node: node.clone() });
            let mut names = Vec::new();
            let ok = match g.scene.tree_for(r, view) {
                Some(t) => match find(t, &node) {
                    Some(i) => {
                        let mut gone: HashSet<usize> = descendants(t, Some(i)).into_iter().collect();
                        gone.insert(i);
                        names.extend(gone.iter().map(|&k| t[k].name.clone()));
                        let keep: Vec<usize> = (0..t.len()).filter(|k| !gone.contains(k)).collect();
                        let remap: HashMap<usize, usize> = keep.iter().enumerate().map(|(n, &o)| (o, n)).collect();
                        let mut nt: Vec<SceneNode> = keep.iter().map(|&k| t[k].clone()).collect();
                        for n in &mut nt {
                            n.parent = n.parent.and_then(|p| remap.get(&p).copied());
                        }
                        *t = nt;
                        true
                    }
                    None => false,
                },
                None => false,
            };
            if ok {
                g.scene.taken_out(r, view, names);
                g.scene.queue_targeted(view, SceneEdit::Remove { r, node }, target);
            }
            XVal::Num(f64::from(u8::from(ok)))
        }
        // `ref add "Target|Name"`: a new node under Target; "^Name" puts it above Target
        // [documented: InsertNode; observed: both return 1 and show in the walks].
        "insertnode" | "attachmodel" => {
            let target_ref = form(args, 0).unwrap_or(r);
            // Modes 1 and 3 insert, 0 and 2 remove [documented: InsertNode's addOrRemove
            // table]. InsertNode removal remains a follow-up; the attachment producer
            // separately consumes modes 0/2 with exact instance ownership.
            let mode = num(args, 1, 1.0);
            let add = mode as i64 & 1 == 1;
            let spec = text(args, 2);
            let Some((target, what)) = spec.split_once('|') else { return Some(Ok(zero)) };
            // JIPPPLN's runtime node lists (`crate::nvse_jippp`) keep every well-formed call.
            crate::nvse_jippp::record(g, target_ref, name == "attachmodel", &spec, mode);
            if name == "attachmodel" && mode.is_finite() && (0.0..=3.0).contains(&mode) && mode.fract() == 0.0 {
                g.scene.attachment_change(target_ref, view, target, what, mode as u8);
            }
            if !add {
                return Some(Ok(zero));
            }
            if name == "insertnode" {
                // Into every drawn model of the reference, the player's both; a model
                // drawn later gets it from the runtime node list
                // (`Game::insert_runtime_nodes`) [hypothesis: B42 True Leaning inserts
                // under Camera1st, a node of the first-person skeleton only, with no model
                // named; observed: mods_reference ln_B42Leaning.txt].
                let (as_parent, nm) = match what.strip_prefix('^') {
                    Some(n) => (true, n),
                    None => (false, what),
                };
                g.scene.asked_again(target_ref, nm);
                return Some(Ok(XVal::Num(f64::from(u8::from(g.scene.insert_node(target_ref, target, nm, as_parent))))));
            }
            let Some(t) = g.scene.tree_for(target_ref, view) else { return Some(Ok(zero)) };
            if find(t, target).is_none() {
                return Some(Ok(zero));
            }
            g.scene.queue_for(view, SceneEdit::Attach { r: target_ref, target: target.to_string(), model: what.to_string() });
            XVal::Num(1.0)
        }
        // ---- JIP batch 9 (docs/spec-notes/jip-ui-9.md §3)
        // `block id`: a value of the block's NiMaterialProperty, 0 without one, a block
        // of that name or 3D [hypothesis: the page names no failure value].
        "getmaterialpropertyvalue" => {
            let id = num(args, 1, -1.0);
            let Some(t) = g.scene.tree_for(r, view) else { return Some(Ok(zero)) };
            let v = find(t, &text(args, 0)).and_then(|i| t[i].material).filter(|_| (0.0..9.0).contains(&id)).map_or(0.0, |m| m[id as usize]);
            XVal::Num(f64::from(v))
        }
        // `block id value`: the value, for the engine to draw; this reference's only
        // [hypothesis: the page's note says an inventory reference changes the base's
        // model]. 0 [hypothesis: as the other setters, though the page names a result].
        "setmaterialpropertyvalue" => {
            let (node, id, v) = (text(args, 0), num(args, 1, -1.0), num(args, 2, f64::NAN));
            if !(0.0..9.0).contains(&id) || !v.is_finite() {
                return Some(Ok(zero));
            }
            let (id, v) = (id as usize, v as f32);
            let mut found = false;
            if let Some(t) = g.scene.tree_for(r, view) {
                if let Some(m) = find(t, &node).and_then(|i| t[i].material.as_mut()) {
                    m[id] = v;
                    found = true;
                }
            }
            if found {
                g.scene.queue_for(view, SceneEdit::Material { r, node, id, v });
            }
            zero
        }
        // `block [getWorld] [pcModel]` (the three variables taken out by the VM): 1 and
        // the translation, or 0 and the variables unchanged [documented]. The world
        // position adds the nodes above and the reference's own place [hypothesis: the
        // reference's angles turn its model as a node's do]. The selected player
        // model uses its own mirror.
        "getnifblocktranslationalt" => {
            let world = num(args, 1, 0.0) != 0.0;
            let Some((pos, _)) = node_place_for(g, r, &text(args, 0), world, view) else { return Some(Ok(outputs(g, 0.0, &[]))) };
            outputs(g, 1.0, &pos.map(f64::from))
        }
        // `block [which] [pcModel]`: 0 extrinsic to the parent (as GetNifBlockRotation),
        // 1 extrinsic in the world, 2/3 decompose the transpose (`m`, the drawn rotation
        // matrix) with the same ZYX Euler extraction [documented: the page's table;
        // observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
        // 2026-11-24, `w6_t1_04_nif_trans_rot_alt`
        // (`tools/capture/nvse/catalogs/wave6_t1_nif_rot_alt.json`, `jip-ui-9.md` §3):
        // `r0=(-1.9,0.4,23.5) -> r2=(1.9,0.4,-23.5)` and
        // `r1=(89.0,83.1,80.7) -> r3=(-89.2,-81.7,82.3)`].
        "getnifblockrotationalt" => {
            let which = num(args, 1, 0.0) as i64;
            let Some((_, m)) = node_place_for(g, r, &text(args, 0), which == 1 || which == 3, view) else { return Some(Ok(outputs(g, 0.0, &[]))) };
            let a = if which >= 2 { extrinsic_angles(&m) } else { extrinsic_angles(&transpose(&m)) };
            outputs(g, 1.0, &a.map(f64::from))
        }
        // `form block`: the form's model has a node of that exact name [observed:
        // "projectilenode" 0 where "ProjectileNode" is 1].
        "modelhasblock" => {
            let (Some(f), want) = (form(args, 0), text(args, 1)) else { return Some(Ok(zero)) };
            // A published reference tree includes instance-only nodes absent from
            // MODL. Its exact-name answer, even for an empty tree, is authoritative
            // (nif-player-model-selection.md, ModelHasBlock). Keep file fallback for
            // base forms and references awaiting their first publication.
            let reference = f == FormId::PLAYER_REF
                || crate::nvse_jg_nif::is_camera_overlay_ref(f)
                || g.world.placed.get(f).is_some()
                || g.world.peek(f).is_some();
            let published = if reference { g.scene.tree(f).map(|nodes| !want.is_empty() && nodes.iter().any(|node| node.name == want)) } else { None };
            let found = published.unwrap_or_else(|| model_of(g, f).and_then(|m| crate::mod_files::read_data(&format!("meshes\\{m}"))).is_some_and(|bytes| crate::mod_files::nif_has_name(&bytes, &want)));
            XVal::Num(f64::from(u8::from(found)))
        }
        _ => return None,
    };
    Some(Ok(v))
}
