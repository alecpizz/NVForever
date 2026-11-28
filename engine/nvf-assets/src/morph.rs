//! Vertex morphs on models (docs/spec-notes/vertex-morphs.md): a shape's
//! `NiGeomMorpherController` blends its `NiMorphData` targets by weights over time
//! (waving flags, sagging wires, the Big MT's holograms).
//!
//! A morphing shape's mesh is kept in the main world too (its usage includes
//! `MAIN_WORLD`), registered here by asset id when the model is prepared
//! (`ModelCtx::prepare`), and rewritten every frame from the shared clock: every placed
//! copy of a model shares its free-morph meshes, so they all wave in step [hypothesis: the original's
//! controllers run on the game's clock the same way; no capture has compared two flags].
//! Normals are left as stored: every morph controller of the base archives has
//! `morpher_flags` UPDATE_NORMALS_DISABLED [observed: `examples/morph_survey.rs`;
//! documented: nif.xml GeomMorpherFlags].
//! [design, #2685] Manager-controlled morphs instead use private meshes and the
//! active rig sequence clock (`play_sequence_morphs`), including its held endpoint.

use crate::anim_play::{OwnCycle, Playing};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use fnv_nif::anim::{GeomMorpher, Sequence, ValueTrack};
use std::collections::HashSet;
use std::sync::Arc;

/// [design, #2685] Manager morphs retain each named sequence's weights, including
/// non-repeating CLAMP tracks. Their time comes directly from the rig's Playing.
#[derive(Debug)]
pub struct SequenceMorph {
    shape: String,
    sequences: Vec<(String, GeomMorpher)>,
}

impl SequenceMorph {
    pub fn bind(g: &GeomMorpher, sequences: &[Sequence], shape: &str) -> Option<Self> {
        if g.flags & 0x20 == 0 || !g.relative || g.morphs.len() < 2 || g.morphs.iter().any(|m| m.vectors.len() != g.num_vertices) {
            return None;
        }
        let sequences: Vec<_> = sequences.iter().filter_map(|s| {
            let name = s.name.as_ref()?;
            let mut bound = g.clone();
            (bound.take_sequence(s, shape) > 0 && bound.moves()).then(|| (name.clone(), bound))
        }).collect();
        (!sequences.is_empty()).then(|| Self { shape: shape.into(), sequences })
    }

    pub fn num_vertices(&self) -> usize {
        self.sequences[0].1.num_vertices
    }
}

/// [design, #2685] One rig shape's private mesh and last written positions. The
/// source stays shared until the first sample, then only this entity's copy changes.
#[derive(Component)]
pub struct PlayingMorph {
    root: Entity,
    morph: Arc<SequenceMorph>,
    own: bool,
    last: Vec<[f32; 3]>,
    logged: HashSet<String>,
}

impl PlayingMorph {
    pub fn new(root: Entity, morph: Arc<SequenceMorph>) -> Self {
        Self { root, morph, own: false, last: Vec::new(), logged: HashSet::new() }
    }
}

/// [design, #2685] Sample after the rig advances: held and restored CLAMP endpoints
/// use the same sequence_time as alpha/UV. Independent references never share writes.
pub fn play_sequence_morphs(
    players: Query<&Playing>,
    mut shapes: Query<(&mut Mesh3d, &mut PlayingMorph)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (mut handle, mut state) in &mut shapes {
        let Ok(p) = players.get(state.root) else { continue };
        let Some((_, morph)) = state.morph.sequences.iter().find(|(name, _)| name.eq_ignore_ascii_case(&p.name)) else { continue };
        let mut positions = Vec::new();
        morph.positions_at(p.sequence_time(), &mut positions);
        if state.own && positions == state.last {
            continue;
        }
        if !state.own {
            let Some(copy) = meshes.get(&handle.0).cloned() else { continue };
            handle.0 = meshes.add(copy);
            state.own = true;
        }
        let Some(mut mesh) = meshes.get_mut(&handle.0) else { continue };
        if positions.len() != mesh.count_vertices() {
            continue;
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, VertexAttributeValues::Float32x3(positions.clone()));
        state.last = positions;
        if state.logged.insert(p.name.clone()) {
            info!("vertex morphs: {} {} plays {} on rig clock", p.source, state.morph.shape, p.name);
        }
    }
}

/// One morphing shape: its stored vertices, the later morphs' offsets with their
/// weights, and the timing they run on.
#[derive(Debug)]
pub struct ShapeMorph {
    /// `<model path> <shape name>`, for the log.
    pub label: String,
    /// Morph 0, the shape's stored vertices.
    pub base: Vec<[f32; 3]>,
    /// Each later morph's offsets and its weight over time (`None`: weight 0).
    pub offsets: Vec<(Vec<[f32; 3]>, Option<ValueTrack>)>,
    pub cycle: OwnCycle,
}

impl ShapeMorph {
    /// The weights at `clock` seconds of play.
    pub fn weights(&self, clock: f32) -> Vec<f32> {
        let t = self.cycle.time(clock);
        self.offsets.iter().map(|(_, w)| w.as_ref().and_then(|w| w.at(t)).map_or(0.0, |v| v[0])).collect()
    }

    /// The vertex positions for `weights`: the base plus each morph times its weight
    /// (relative targets [hypothesis], docs/spec-notes/vertex-morphs.md §2).
    pub fn positions(&self, weights: &[f32]) -> Vec<[f32; 3]> {
        let mut out = self.base.clone();
        for ((offsets, _), &w) in self.offsets.iter().zip(weights) {
            if w == 0.0 {
                continue;
            }
            for (p, v) in out.iter_mut().zip(offsets) {
                p[0] += w * v[0];
                p[1] += w * v[1];
                p[2] += w * v[2];
            }
        }
        out
    }
}

/// The meshes that morph (kept in [`crate::ModelCache`]), by asset id (weak: a mesh nothing holds any more is dropped
/// from the list), with the weights they were last written for.
#[derive(Default)]
pub struct MorphMeshes {
    entries: Vec<(AssetId<Mesh>, Arc<ShapeMorph>, Vec<f32>)>,
    logged: HashSet<String>,
    /// Meshes rewritten, summed over frames (the `--run-seconds` summary).
    pub writes: u64,
}

impl MorphMeshes {
    pub fn add(&mut self, mesh: AssetId<Mesh>, morph: Arc<ShapeMorph>) {
        self.entries.push((mesh, morph, Vec::new()));
    }

    /// Morphing meshes registered and still present.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The registered meshes (nvforever's profile aid, `profile change sources:`).
    pub fn ids(&self) -> impl Iterator<Item = AssetId<Mesh>> + '_ {
        self.entries.iter().map(|(id, ..)| *id)
    }
}

/// Rewrite every registered mesh whose weights changed since its last write.
pub fn play_vertex_morphs(time: Res<Time>, mut cache: ResMut<crate::ModelCache>, mut meshes: ResMut<Assets<Mesh>>) {
    let clock = time.elapsed_secs();
    let MorphMeshes { entries, logged, writes } = &mut cache.morphs;
    entries.retain_mut(|(id, morph, last)| {
        // Read the weights first: a mesh unchanged is not touched (no re-upload).
        let w = morph.weights(clock);
        if *last == w {
            return meshes.contains(*id);
        }
        let Some(mut mesh) = meshes.get_mut(*id) else { return false };
        let positions = morph.positions(&w);
        if positions.len() != mesh.count_vertices() {
            return false;
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, VertexAttributeValues::Float32x3(positions));
        if !last.is_empty() && logged.insert(morph.label.clone()) {
            info!("vertex morphs: {} running ({} morph(s), weights {:?})", morph.label, w.len(), w.iter().map(|x| (x * 100.0).round() / 100.0).collect::<Vec<_>>());
        }
        *last = w;
        *writes += 1;
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_nif::anim::{Channel, Key, KeyKind, Keys};

    #[test]
    fn forward_morph_uses_rig_clock_and_holds_restored_end_per_instance() {
        use crate::anim_play::Clip;
        use crate::model::{Rig, RigNode, RigShape};
        use fnv_nif::anim::{MorphTarget, Track, TrackData};
        let ramp = Keys { kind: KeyKind::Linear, keys: [3.0, 7.0].into_iter().enumerate().map(|(i, time)| Key {
            time, value: [i as f32], forward: [0.0], backward: [0.0], tbc: [0.0; 3],
        }).collect() };
        let seq = Sequence {
            block: 0, name: Some("Forward".into()), start_time: 3.0, stop_time: 7.0,
            frequency: 2.0, cycle_type: 2, accum_root_name: None, text_keys: Vec::new(),
            tracks: vec![Track { node_name: Some("OwnedFog".into()), controller_type: Some("NiGeomMorpherController".into()),
                controller_id: None, interpolator_id: Some("OwnedFrame".into()), priority: None,
                data: TrackData::Other("OwnedSynthetic".into()), values: Some(ValueTrack::Float(Channel::Keys(ramp))), bools: None }],
        };
        let target = |name: &str, vector| MorphTarget {
            name: Some(name.into()), vectors: vec![vector], weight: None, stored_weight: 0.0, interpolator_type: None,
        };
        let g = GeomMorpher {
            controller: 1, target: 0, flags: 0x20, start_time: 0.0, stop_time: 0.0,
            frequency: 1.0, phase: 0.0, update_normals: false, relative: true, num_vertices: 1,
            morphs: vec![target("Base", [1.0, 0.0, 0.0]), target("OwnedFrame", [0.0, 4.0, 0.0])],
        };
        assert!(SequenceMorph::bind(&g, std::slice::from_ref(&seq), "Other").is_none());
        let morph = Arc::new(SequenceMorph::bind(&g, std::slice::from_ref(&seq), "OwnedFog").unwrap());
        let clip = Clip::from_sequence(&seq);
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>().add_systems(Update, play_sequence_morphs);
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::all());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[1.0, 0.0, 0.0]]);
        let source = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let rig = Rig {
            nodes: vec![RigNode { name: Some("OwnedFog".into()), parent: None, local: Transform::IDENTITY, hidden: false, billboard: None }],
            shapes: vec![RigShape { node: 0, mesh: source.clone(), material: default(), skin: None,
                look: ("OwnedFog".into(), [0.0; 3], 1.0, 1.0), refraction: 0.0, animated: false, morph: Some(morph), block: 0 }],
            idle: None, door: None, groups: vec![clip.clone()], effect: None,
        };
        let parent = app.world_mut().spawn_empty().id();
        let mut spawn = || {
            let (root, _, _) = rig.spawn_nodes(&mut app.world_mut().commands(), parent, Transform::IDENTITY, "owned.nif", 0.0);
            app.world_mut().flush();
            root
        };
        let a = spawn();
        let b = spawn();
        let position = |app: &App, e| {
            let handle = &app.world().get::<Mesh3d>(e).unwrap().0;
            let meshes = app.world().resource::<Assets<Mesh>>();
            match meshes.get(handle).unwrap().attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
                VertexAttributeValues::Float32x3(v) => v[0],
                _ => panic!("positions must be Float32x3"),
            }
        };
        // [design, #2685] An inactive group keeps the source pose until PlayGroup.
        app.update();
        assert_eq!(position(&app, a), [1.0, 0.0, 0.0]);
        app.world_mut().entity_mut(a).insert(Playing::new("owned.nif", &clip, |_| None));
        let mut restored = Playing::new("owned.nif", &clip, |_| None);
        restored.time = clip.stop;
        restored.held = true;
        app.world_mut().entity_mut(b).insert(restored);
        app.update();
        assert_eq!(position(&app, a), [1.0, 0.0, 0.0]);
        assert_eq!(position(&app, b), [1.0, 4.0, 0.0]);
        assert_ne!(app.world().get::<Mesh3d>(a).unwrap().0, app.world().get::<Mesh3d>(b).unwrap().0);
        app.world_mut().get_mut::<Playing>(a).unwrap().time = 5.0;
        app.update();
        assert_eq!(position(&app, a), [1.0, 2.0, 0.0]);
        assert_eq!(position(&app, b), [1.0, 4.0, 0.0]);
        app.world_mut().get_mut::<Playing>(a).unwrap().time = 100.0;
        app.world_mut().get_mut::<Playing>(a).unwrap().held = true;
        app.update();
        app.update();
        assert_eq!(position(&app, a), [1.0, 4.0, 0.0]);
        let meshes = app.world().resource::<Assets<Mesh>>();
        assert!(matches!(meshes.get(&source).unwrap().attribute(Mesh::ATTRIBUTE_POSITION), Some(VertexAttributeValues::Float32x3(v)) if v[0] == [1.0, 0.0, 0.0]));
    }

    #[test]
    fn base_plus_weighted_offsets() {
        let ramp = Keys { kind: KeyKind::Linear, keys: vec![Key { time: 0.0, value: [0.0], forward: [0.0], backward: [0.0], tbc: [0.0; 3] }, Key { time: 2.0, value: [1.0], forward: [0.0], backward: [0.0], tbc: [0.0; 3] }] };
        let m = ShapeMorph {
            label: "flag.nif Flag:0".into(),
            base: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            offsets: vec![(vec![[0.0, 0.0, 0.0], [0.0, 4.0, 0.0]], Some(ValueTrack::Float(Channel::Keys(ramp)))), (vec![[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], None)],
            cycle: OwnCycle { start: 0.0, stop: 2.0, frequency: 1.0, phase: 0.0, cycle_type: 0 },
        };
        // Half way through the loop, and again one cycle later.
        for clock in [1.0, 3.0] {
            let w = m.weights(clock);
            assert!((w[0] - 0.5).abs() < 1e-5 && w[1] == 0.0);
            assert_eq!(m.positions(&w), vec![[0.0, 0.0, 0.0], [10.0, 2.0, 0.0]]);
        }
    }
}
