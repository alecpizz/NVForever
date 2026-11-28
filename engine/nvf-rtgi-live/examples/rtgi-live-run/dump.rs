//! A scene dump (`nvf.scene-dump/1`) as the solver takes it.

use crate::Result;
use nvf_rtgi::{
    prepare, Cone, Cull, Directional, Floor, FloorLimits, Instance, Lamp, Level, LightState, Material, Mesh, PreparedScene, Room,
    SceneInput, SceneLimits, Seeds, Texture, Wrap, V3,
};
use serde::Deserialize;
use std::path::Path;
use std::time::Instant;

fn read(path: &Path) -> Result<Vec<u8>> {
    Ok(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
}

/// What a dump's `scene.json` has that a solve needs.
mod format {
    use super::Deserialize;

    #[derive(Deserialize, Clone, Copy)]
    pub struct Block {
        pub offset: u64,
        pub bytes: u64,
    }

    #[derive(Deserialize)]
    pub struct Ambient {
        pub display: [f32; 3],
    }

    #[derive(Deserialize)]
    pub struct Directional {
        pub display: [f32; 3],
        pub to_source: [f32; 3],
        pub on: bool,
    }

    #[derive(Deserialize)]
    pub struct Light {
        /// `point`, `spot` or `dormant`.
        pub kind: String,
        pub on: bool,
        pub display: [f32; 3],
        pub range: f32,
        pub position: [f32; 3],
        pub direction: [f32; 3],
        pub inner_angle: Option<f32>,
        pub outer_angle: Option<f32>,
    }

    #[derive(Deserialize)]
    pub struct Instance {
        pub mesh: usize,
        pub material: usize,
        pub world_from_local: [[f32; 4]; 3],
    }

    #[derive(Deserialize)]
    pub struct Mesh {
        pub positions: Block,
        pub indices: Block,
        pub normals: Option<Block>,
        pub uv0: Option<Block>,
        pub colors: Option<Block>,
    }

    #[derive(Deserialize)]
    pub struct Material {
        pub base_color: [f32; 4],
        pub texture: Option<usize>,
        pub alpha_mode: String,
        pub cutoff: f32,
        pub alpha_cut: f32,
        pub double_sided: bool,
        pub cull: Option<String>,
        pub unlit: bool,
        pub vertex_colors_display: bool,
        pub uv_transform: [[f32; 3]; 2],
    }

    /// A missing one has its path alone.
    #[derive(Deserialize, Default)]
    #[serde(default)]
    pub struct Texture {
        pub missing: bool,
        pub format: String,
        pub srgb: bool,
        pub width: u32,
        pub height: u32,
        pub mips: u32,
        pub address: Vec<String>,
        pub data: Option<Block>,
    }

    #[derive(Deserialize)]
    pub struct Room {
        pub center: [f32; 3],
        pub axes: [[f32; 3]; 3],
        pub half: [f32; 3],
    }

    #[derive(Deserialize)]
    pub struct NavMesh {
        pub disabled: bool,
        pub vertices: Block,
        pub triangles: Block,
    }

    #[derive(Deserialize)]
    pub struct Doc {
        pub schema: String,
        pub bin: String,
        pub ambient: Ambient,
        pub directional: Vec<Directional>,
        pub lights: Vec<Light>,
        pub instances: Vec<Instance>,
        pub meshes: Vec<Mesh>,
        pub materials: Vec<Material>,
        pub textures: Vec<Texture>,
        pub rooms: Vec<Room>,
        pub nav: Vec<NavMesh>,
    }
}

/// The files of a dump.
struct Dump {
    doc: format::Doc,
    bin: Vec<u8>,
}

impl Dump {
    fn load(folder: &Path) -> Result<Dump> {
        let doc: format::Doc = serde_json::from_slice(&read(&folder.join("scene.json"))?).map_err(|e| format!("scene.json: {e}"))?;
        if doc.schema != "nvf.scene-dump/1" {
            return Err(format!("schema {:?}", doc.schema).into());
        }
        let bin_name = Path::new(&doc.bin);
        if bin_name.file_name() != Some(bin_name.as_os_str()) {
            return Err("the binary must be a file in the dump folder".into());
        }
        let bin = read(&folder.join(bin_name))?;
        Ok(Dump { doc, bin })
    }

    fn bytes(&self, b: &format::Block) -> Result<&[u8]> {
        let end = b.offset.checked_add(b.bytes).ok_or("a block past the end")?;
        self.bin
            .get(usize::try_from(b.offset)?..usize::try_from(end)?)
            .ok_or_else(|| format!("a block of {} bytes at {}", b.bytes, b.offset).into())
    }

    fn words(&self, b: &format::Block) -> Result<Vec<u32>> {
        let (words, tail) = self.bytes(b)?.as_chunks::<4>();
        if !tail.is_empty() {
            return Err(format!("partial word in block at {}", b.offset).into());
        }
        Ok(words.iter().map(|w| u32::from_le_bytes(*w)).collect())
    }

    fn floats<const N: usize>(&self, b: &format::Block) -> Result<Vec<[f32; N]>> {
        let words = self.words(b)?;
        let (rows, tail) = words.as_chunks::<N>();
        if !tail.is_empty() {
            return Err(format!("partial row in block at {}", b.offset).into());
        }
        Ok(rows.iter().map(|w| w.map(f32::from_bits)).collect())
    }

    fn some<const N: usize>(&self, b: &Option<format::Block>) -> Result<Vec<[f32; N]>> {
        b.as_ref().map_or_else(|| Ok(Vec::new()), |b| self.floats(b))
    }

    fn texture(&self, t: &format::Texture) -> Result<Option<Texture>> {
        Ok(match &t.data {
            Some(b) if !t.missing => decode(t, self.bytes(b)?),
            _ => None,
        })
    }

    fn scene(&self) -> Result<SceneInput> {
        let d = &self.doc;
        let mesh = |m: &format::Mesh| {
            Ok(Mesh {
                positions: self.floats(&m.positions)?,
                normals: self.some(&m.normals)?,
                uv0: self.some(&m.uv0)?,
                colors: self.some(&m.colors)?,
                indices: self.words(&m.indices)?,
            })
        };
        Ok(SceneInput {
            meshes: d.meshes.iter().map(mesh).collect::<Result<_>>()?,
            instances: d
                .instances
                .iter()
                .map(|x| Instance { mesh: x.mesh, material: x.material, world_from_local: x.world_from_local })
                .collect(),
            materials: d.materials.iter().map(material).collect(),
            textures: d.textures.iter().map(|t| self.texture(t)).collect::<Result<_>>()?,
        })
    }

    fn light(&self) -> Result<LightState> {
        let d = &self.doc;
        let mut lamps = Vec::new();
        for (i, l) in d.lights.iter().enumerate().filter(|(_, l)| l.on && l.kind != "dormant") {
            let cone = if l.kind == "spot" {
                let (inner, outer) = l.inner_angle.zip(l.outer_angle)
                    .ok_or_else(|| format!("lights[{i}]: an active spot requires both inner_angle and outer_angle"))?;
                Some(Cone::new(V3(l.direction), inner, outer))
            } else {
                None
            };
            lamps.push(Lamp { position: V3(l.position), display: V3(l.display), range: l.range, cone });
        }
        Ok(LightState {
            ambient: V3(d.ambient.display),
            directionals: d
                .directional
                .iter()
                .filter(|l| l.on)
                .map(|l| Directional { display: V3(l.display), to_source: V3(l.to_source) })
                .collect(),
            lamps,
        })
    }

    /// The floor's triangles, and the rooms.
    fn floor(&self) -> Result<(Vec<[V3; 3]>, Vec<Room>)> {
        let mut triangles = Vec::new();
        for n in self.doc.nav.iter().filter(|n| !n.disabled) {
            let points = self.floats::<3>(&n.vertices)?;
            let indices = self.words(&n.triangles)?;
            let (faces, tail) = indices.as_chunks::<3>();
            if !tail.is_empty() {
                return Err("a partial navmesh triangle".into());
            }
            for t in faces {
                let corner = |i: u32| points.get(i as usize).map(|p| V3(*p)).ok_or("a navmesh's index");
                triangles.push([corner(t[0])?, corner(t[1])?, corner(t[2])?]);
            }
        }
        let rooms: Vec<Room> = self.doc.rooms.iter().map(|r| Room { center: V3(r.center), axes: r.axes.map(V3), half: r.half }).collect();
        Ok((triangles, rooms))
    }
}

fn material(m: &format::Material) -> Material {
    Material {
        base: m.base_color,
        texture: m.texture,
        // The shader's two tests (nif_material.wesl: alpha_cut, then alpha_discard).
        cutoff: if m.alpha_mode == "mask" { m.cutoff.max(m.alpha_cut) } else { m.alpha_cut },
        cull: match m.cull.as_deref() {
            Some("back") => Cull::Back,
            Some("front") => Cull::Front,
            _ => Cull::None,
        },
        double_sided: m.double_sided,
        unlit: m.unlit,
        display_colors: m.vertex_colors_display,
        uv: m.uv_transform,
    }
}

/// The first layer; `None` for an unknown format or data cut short. Levels that are not read are left out.
fn decode(t: &format::Texture, data: &[u8]) -> Option<Texture> {
    let (side, block, format) = match t.format.as_str() {
        "Bc1RgbaUnorm" | "Bc1RgbaUnormSrgb" => (4, 8, Some(texpresso::Format::Bc1)),
        "Bc2RgbaUnorm" | "Bc2RgbaUnormSrgb" => (4, 16, Some(texpresso::Format::Bc2)),
        "Bc3RgbaUnorm" | "Bc3RgbaUnormSrgb" => (4, 16, Some(texpresso::Format::Bc3)),
        "Rgba8Unorm" | "Rgba8UnormSrgb" => (1, 4, None),
        _ => return None,
    };
    let (mut levels, mut at) = (Vec::new(), 0usize);
    let mips = t.mips.min(32);
    for m in 0..mips {
        let dim = |s: u32| s.checked_shr(m).unwrap_or(0).max(1);
        let (w, h) = (dim(t.width) as usize, dim(t.height) as usize);
        let bytes = w.div_ceil(side).checked_mul(h.div_ceil(side))?.checked_mul(block)?;
        let end = at.checked_add(bytes)?;
        let source = data.get(at..end)?;
        at = end;
        if w.max(h) > 256 && m + 1 < mips {
            continue;
        }
        let texels = if let Some(f) = format {
            let mut rgba = vec![[0; 4]; w.checked_mul(h)?];
            f.decompress(source, w, h, rgba.as_flattened_mut());
            rgba
        } else {
            source.as_chunks::<4>().0.to_vec()
        };
        levels.push(Level { width: w, height: h, texels });
    }
    // `Default` is the engine's default sampler, which clamps.
    let wrap = |i: usize| match t.address.get(i).map(String::as_str) {
        Some("Repeat") => Wrap::Repeat,
        Some("MirrorRepeat") => Wrap::Mirror,
        _ => Wrap::Clamp,
    };
    Some(Texture { levels, srgb: t.srgb, wrap: [wrap(0), wrap(1)] })
}

pub struct Capture {
    pub scene: PreparedScene,
    pub light: LightState,
    pub seeds: Seeds,
    pub load_ms: f64,
    pub prepare_ms: f64,
}

pub fn loaded(folder: &Path, threads: usize) -> Result<Capture> {
    let start = Instant::now();
    let dump = Dump::load(folder)?;
    let (input, light, (triangles, rooms)) = (dump.scene()?, dump.light()?, dump.floor()?);
    drop(dump);
    let load_ms = crate::ms(start);
    let start = Instant::now();
    let floor = Floor::new(&triangles, &rooms, &FloorLimits::default(), &|| false)?;
    let seeds = Seeds::of(floor);
    let scene = prepare(input, &SceneLimits::default(), threads, &|| false)?;
    let prepare_ms = crate::ms(start);
    Ok(Capture { scene, light, seeds, load_ms, prepare_ms })
}
