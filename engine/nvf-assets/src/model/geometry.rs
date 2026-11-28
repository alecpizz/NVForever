//! A drawn mesh's arrays out of its model's file a second time (docs/design/rtgi-collect.md):
//! the meshes themselves go to the GPU and are gone from the main world. Which file and which
//! of its shapes a mesh is ([`ModelCache::sources`]), and the shapes' arrays as the loader
//! makes them ([`geometry`]), in plain data. Nothing in the game calls it yet.
//!
//! [design] A shape is named by its block, which is the same in every variant of a model and
//! does not depend on which shapes the loader leaves out. Read again, a file is taken to be what
//! it was when it was loaded: a loose file that changed since and kept the block, its name and
//! its box ([`Geometry::same_box`]) is not told from it.

use super::ModelCache;
use crate::nif_spawn::{fix_vertex_colors, shape_props, AdmittedShape, NumericRejections};
use bevy::shape::Aabb3d;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use fnv_nif::scene::{scene_objects, Decoded, SceneObject};
use fnv_nif::{NifFile, Schema};
use std::collections::{HashMap, HashSet};

/// The file and the shape a mesh of the model cache was made from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshSource {
    /// The model's path in the VFS.
    pub path: String,
    /// The shape's block in that file.
    pub block: u32,
    /// The shape's name, empty when it has none.
    pub name: String,
}

/// A shape's arrays, in its own space.
#[derive(Clone, Debug)]
pub struct Geometry {
    /// Those that a scene for light rays takes; the order of the corners and the triangles is
    /// the drawn mesh's.
    pub mesh: nvf_rtgi::Mesh,
    bounds: Option<Aabb3d>,
}

impl Geometry {
    fn of(mut mesh: Mesh) -> Result<Geometry, String> {
        // Taken as the renderer takes a mesh, which is what sets the box of the drawn one too.
        let mut mesh = mesh.take_gpu_data().map_err(|e| e.to_string())?;
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.remove_attribute(Mesh::ATTRIBUTE_POSITION) else { return Err("positions of another kind".into()) };
        let Some(Indices::U32(indices)) = mesh.remove_indices() else { return Err("indices of another kind".into()) };
        // Its triangles may all be of corners that are not there: the loader draws it empty.
        if indices.is_empty() {
            return Err("nothing to draw".into());
        }
        let mut out = nvf_rtgi::Mesh { positions, indices, ..default() };
        if let Some(VertexAttributeValues::Float32x3(v)) = mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL) {
            out.normals = v;
        }
        if let Some(VertexAttributeValues::Float32x2(v)) = mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0) {
            out.uv0 = v;
        }
        if let Some(VertexAttributeValues::Float32x4(v)) = mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR) {
            out.colors = v;
        }
        Ok(Geometry { mesh: out, bounds: mesh.final_aabb })
    }

    /// Whether the positions' box is `drawn`, a drawn mesh's `final_aabb`, bit for bit. True
    /// where that one has none: a mesh that stays on the CPU.
    pub fn same_box(&self, drawn: Option<Aabb3d>) -> bool {
        let bits = |b: Aabb3d| [b.min, b.max].map(|p| p.to_array().map(f32::to_bits));
        drawn.is_none_or(|drawn| self.bounds.map(bits) == Some(bits(drawn)))
    }
}

/// The arrays of the shapes `wanted` (block and name, [`MeshSource`]) of the model in `bytes`, in
/// that order: what the loader (`prepare_unguarded`) puts into the meshes it draws, skinned
/// shapes in their bind pose. It makes nothing else: no handle, material, tangent or clip, and
/// it counts and logs nothing. Whether the loader draws such a shape is not asked (a drawn mesh
/// is what its source was asked for). An error for the file when one of them is not there, has
/// another name or draws nothing, and for a panic in a broken file.
pub fn geometry(bytes: &[u8], schema: &Schema, wanted: &[(u32, &str)]) -> Result<Vec<Geometry>, String> {
    crate::guard::guarded(|| {
        let file = NifFile::parse(bytes, Some(schema)).map_err(|e| e.to_string())?;
        let dec = Decoded::new(&file, bytes, schema);
        let objects = scene_objects(&file, &dec);
        let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
        let read = |&(block, name): &(u32, &str)| shape(&file, &dec, &by_block, block, name).map_err(|e| format!("block {block} ({name}): {e}"));
        wanted.iter().map(read).collect()
    })
}

fn shape(file: &NifFile, dec: &Decoded, by_block: &HashMap<usize, &SceneObject>, block: u32, name: &str) -> Result<Geometry, String> {
    let o = by_block.get(&(block as usize)).filter(|o| !o.is_node).ok_or("no shape")?;
    let has = o.name.as_deref().unwrap_or_default();
    if has != name {
        return Err(format!("its name is {has}"));
    }
    let Ok(shape) = &dec.blocks[o.block] else { return Err("not decoded".into()) };
    let data = shape.block_ref("data").and_then(|r| dec.get(r)).ok_or("no data")?;
    let props = shape_props(file, dec, o, by_block);
    // The counts of what is rejected are this value's and go with it.
    let AdmittedShape { mut mesh, .. } = NumericRejections::default().admit(o.block, dec, shape, data).ok_or("nothing to draw")?;
    fix_vertex_colors(&mut mesh, props.vertex_alpha_part());
    Geometry::of(mesh)
}

impl ModelCache {
    /// Where these meshes came from, those that a model held here has among its shapes or its
    /// rig's. `None` for one that two of them claim from different places, which no loader here
    /// makes: it is not known.
    pub fn sources(&self, wanted: &HashSet<AssetId<Mesh>>) -> HashMap<AssetId<Mesh>, Option<MeshSource>> {
        let mut out: HashMap<AssetId<Mesh>, Option<MeshSource>> = HashMap::new();
        for (key, model) in &self.map {
            let Some(model) = model else { continue };
            // The marks of the variants follow the path (`model_key`).
            let path = key.split_once('|').map_or(key.as_str(), |(path, _)| path);
            let shapes = model.shapes.iter().map(|s| (s.mesh.id(), s.block, s.name.as_deref().unwrap_or_default()));
            let rig = model.rig.iter().flat_map(|r| &r.shapes).map(|s| (s.mesh.id(), s.block, s.look.0.as_str()));
            for (id, block, name) in shapes.chain(rig).filter(|(id, ..)| wanted.contains(id)) {
                let same = |s: &MeshSource| s.path == path && s.block == block && s.name == name;
                match out.get_mut(&id) {
                    Some(held) if held.as_ref().is_some_and(same) => {}
                    Some(held) => *held = None,
                    None => {
                        out.insert(id, Some(MeshSource { path: path.into(), block, name: name.into() }));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PreparedModel, PreparedShape};
    use std::sync::Arc;

    fn model(mesh: &Handle<Mesh>, block: u32) -> Option<Arc<PreparedModel>> {
        let shape = PreparedShape {
            mesh: mesh.clone(),
            material: Handle::default(),
            transform: default(),
            name: Some("Wall:0".into()),
            ancestors: Vec::new(),
            node: None,
            own: default(),
            cpu: None,
            segments: Vec::new(),
            external_emittance: false,
            look: crate::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
            missing_diffuse: None,
            block,
        };
        Some(Arc::new(PreparedModel { shapes: vec![shape], ..default() }))
    }

    /// One place under two keys is one source; an id from two places is not known, in whichever
    /// order the map gives them and however often. One that is not asked for is left out.
    #[test]
    fn a_source_is_one_place_only() {
        let mut meshes = Assets::<Mesh>::default();
        let mut made = || meshes.add(Mesh::from(Cuboid::default()));
        let (twice, torn, other) = (made(), made(), made());
        let mut cache = ModelCache::default();
        let a = r"meshes\a.nif";
        let mut put = |key: String, m: &Handle<Mesh>, block| cache.map.insert(key, model(m, block));
        put(format!("{a}|attachment"), &twice, 4);
        put(format!("{a}|actor-idle-00012345"), &twice, 4);
        put(r"meshes\b.nif".into(), &torn, 4);
        put(r"meshes\b.nif|cpu".into(), &torn, 5);
        put(r"meshes\c.nif".into(), &torn, 4);
        put(r"meshes\d.nif".into(), &other, 1);
        cache.map.insert(r"meshes\gone.nif".into(), None);
        let wanted = HashSet::from([twice.id(), torn.id(), made().id()]);
        let source = MeshSource { path: a.into(), block: 4, name: "Wall:0".into() };
        let expected = HashMap::from([(twice.id(), Some(source)), (torn.id(), None)]);
        assert_eq!(cache.sources(&wanted), expected);
    }
}
