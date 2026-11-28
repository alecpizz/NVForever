//! What a scene is made of, as its caller hands it over.

/// The triangles of a cell. Meshes are in object space and may be used by many instances.
#[derive(Clone, Debug, Default)]
pub struct SceneInput {
    pub meshes: Vec<Mesh>,
    /// Their order is the triangles' order, which decides between two at exactly the same distance.
    pub instances: Vec<Instance>,
    pub materials: Vec<Material>,
    /// A missing one is white: the material's colour alone.
    pub textures: Vec<Option<Texture>>,
}

/// A mesh's arrays; a missing one is empty. Triangle list, counter-clockwise front.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv0: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub mesh: usize,
    pub material: usize,
    /// Three rows of four: `world = M * (local, 1)`. One that mirrors keeps its front.
    pub world_from_local: [[f32; 4]; 3],
}

/// The side that is not drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cull {
    None,
    Back,
    Front,
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    /// Linear, with the alpha.
    pub base: [f32; 4],
    /// Of the scene's textures.
    pub texture: Option<usize>,
    /// Fragments below this alpha are not there; 0: no test.
    pub cutoff: f32,
    /// Rays stop at the side that is culled too; it is black.
    pub cull: Cull,
    /// The shading normal turns towards the side that is met.
    pub double_sided: bool,
    /// It shows its own colour and gets no light.
    pub unlit: bool,
    /// Vertex colours are interpolated as display values, then made linear.
    pub display_colors: bool,
    /// Two rows of three: `uv = M * (uv0, 1)`.
    pub uv: [[f32; 3]; 2],
}

impl Default for Material {
    fn default() -> Material {
        let uv = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        Material { base: [1.0; 4], texture: None, cutoff: 0.0, cull: Cull::Back, double_sided: false, unlit: false, display_colors: false, uv }
    }
}

/// How a texture goes on past its edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wrap {
    Repeat,
    Mirror,
    Clamp,
}

/// RGBA8 with its mips, the largest first. No level above 256 texels a side is read when a smaller one
/// is there, so the larger ones may be left out.
#[derive(Clone, Debug)]
pub struct Texture {
    pub levels: Vec<Level>,
    /// Whether the colour goes through the sRGB transfer.
    pub srgb: bool,
    /// Along u and v.
    pub wrap: [Wrap; 2],
}

/// Row by row.
#[derive(Clone, Debug)]
pub struct Level {
    pub width: usize,
    pub height: usize,
    pub texels: Vec<[u8; 4]>,
}
