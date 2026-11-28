//! What is read out of the files a second time (`model::geometry`, `texture::levels`) against
//! what the loader drew, through the loader's own entrances: on the main thread (`Stores`) and
//! on a worker (`Deferred`). The model is written here; it has no game data.
//!
//! The tests take turns, as one of them looks at the process's counters.

use bevy::ecs::system::SystemState;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use nvf_assets::anim_play::Clip;
use nvf_assets::model::geometry::{geometry, Geometry, MeshSource};
use nvf_assets::model::{AltTexture, ModelCtx};
use nvf_assets::texture::levels::{self, Drawn, ImageSource};
use nvf_assets::{GameData, ModelCache, NifMaterial, PreparedModel, TextureCache};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

mod made_nif;
use made_nif::{floats, words};

const NONE: u32 = u32::MAX;
const PATH: &str = r"meshes\room.nif";

fn turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|p| p.into_inner())
}

fn halves(out: &mut Vec<u8>, values: &[u16]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}

/// A NIF's blocks in the layout of `schemas/nif`.
#[derive(Default)]
struct Nif {
    blocks: Vec<(&'static str, Vec<u8>)>,
    strings: Vec<&'static str>,
}

impl Nif {
    fn add(&mut self, kind: &'static str, bytes: Vec<u8>) -> u32 {
        self.blocks.push((kind, bytes));
        self.blocks.len() as u32 - 1
    }

    /// NiAVObject's fields: no extra data, the identity, no collision.
    fn object(&mut self, name: &'static str, flags: u32, controller: u32, properties: &[u32]) -> Vec<u8> {
        self.strings.push(name);
        let mut out = vec![];
        words(&mut out, &[self.strings.len() as u32 - 1, 0, controller, flags]);
        floats(&mut out, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0]);
        words(&mut out, &[properties.len() as u32]);
        words(&mut out, properties);
        words(&mut out, &[NONE]);
        out
    }

    fn node(&mut self, name: &'static str, flags: u32, controller: u32, children: &[u32]) -> Vec<u8> {
        let mut out = self.object(name, flags, controller, &[]);
        words(&mut out, &[children.len() as u32]);
        words(&mut out, children);
        words(&mut out, &[0]); // effects
        out
    }

    fn shape(&mut self, name: &'static str, flags: u32, properties: &[u32], data: u32, skin: u32) -> u32 {
        let mut out = self.object(name, flags, NONE, properties);
        words(&mut out, &[data, skin, 0, NONE]); // no materials, none active
        out.push(0); // material needs update
        self.add("NiTriShape", out)
    }
}

/// How a shape's triangles are stored.
enum Corners<'a> {
    List(&'a [u16]),
    Strip(&'a [u16]),
}

/// NiTriShapeData or NiTriStripsData: four corners of a bent square, `x` further along, with
/// these normals and colours.
fn corners(nif: &mut Nif, x: f32, normals: bool, colors: Option<[[f32; 4]; 4]>, stored: Corners) -> u32 {
    let mut out = vec![];
    words(&mut out, &[0]); // group id
    halves(&mut out, &[4]);
    out.extend([0, 0, 1]); // keep, compress, has vertices
    floats(&mut out, &[x, 0.0, 0.0, x + 100.0, 0.0, 10.0, x, 100.0, -20.0, x + 100.0, 100.0, 40.0]);
    halves(&mut out, &[1]); // one UV set
    out.push(u8::from(normals));
    if normals {
        floats(&mut out, &[0.0, 0.0, 1.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.8, 0.0, -0.6, 0.8]);
    }
    floats(&mut out, &[x + 50.0, 50.0, 0.0, 90.0]); // bounding sphere
    out.push(u8::from(colors.is_some()));
    if let Some(c) = colors {
        floats(&mut out, c.as_flattened());
    }
    floats(&mut out, &[x, 0.0, 1.0, 0.25, 0.0, 1.0, 1.0, -3.5]);
    halves(&mut out, &[0]); // consistency
    words(&mut out, &[NONE]); // additional data
    match stored {
        Corners::List(c) => {
            halves(&mut out, &[c.len() as u16 / 3]);
            words(&mut out, &[c.len() as u32]);
            out.push(1); // has triangles
            halves(&mut out, c);
            halves(&mut out, &[0]); // match groups
            nif.add("NiTriShapeData", out)
        }
        Corners::Strip(c) => {
            halves(&mut out, &[c.len() as u16 - 2, 1, c.len() as u16]); // triangles, strips, length
            out.push(1); // has points
            halves(&mut out, c);
            nif.add("NiTriStripsData", out)
        }
    }
}

/// The head of a shader property: no name, smooth.
fn shader(fields: &[u32]) -> Vec<u8> {
    let mut out = vec![];
    words(&mut out, &[NONE, 0, NONE]);
    halves(&mut out, &[1]);
    words(&mut out, fields);
    floats(&mut out, &[1.0]);
    out
}

/// BSShaderPPLightingProperty with these shader flags and no texture set.
fn lit(nif: &mut Nif, flags: u32) -> u32 {
    let mut out = shader(&[1, flags, 0]);
    words(&mut out, &[3, NONE]);
    floats(&mut out, &[0.0]);
    words(&mut out, &[0]);
    floats(&mut out, &[4.0, 1.0]);
    nif.add("BSShaderPPLightingProperty", out)
}

/// BSShaderNoLightingProperty that takes its vertex alpha, without a texture: a loader of such
/// a shape counts it (`nif_spawn::vertex_alpha_blended_summary`).
fn unlit(nif: &mut Nif) -> u32 {
    let mut out = shader(&[33, 0x8200_0008, 1]);
    words(&mut out, &[3, 0]);
    floats(&mut out, &[1.0, 0.0, 1.0, 0.0]);
    nif.add("BSShaderNoLightingProperty", out)
}

/// Colours out of range and none, and alphas below 1: what `fix_vertex_colors` is for.
const COLORS: [[f32; 4]; 4] = [[0.5, 2.0, -1.0, 0.25], [f32::NAN, 0.0, 0.3, 0.75], [1.0, 1.0, 1.0, f32::NAN], [0.1, 0.2, 0.3, 0.0]];

const LIST: Corners = Corners::List(&[0, 1, 2, 1, 3, 2]);

/// The model: shapes that the loader leaves out before all that it draws, so that neither the
/// file's order nor a list's gives a drawn one's number. With the names of the drawn ones; `gone`
/// are the flags of the one whose triangles are.
fn room(gone: u32) -> (Vec<u8>, [&'static str; 7]) {
    let mut nif = Nif::default();
    nif.add("BSFadeNode", vec![]);
    let (plain, faded, dark) = (lit(&mut nif, 0), lit(&mut nif, 1 << 3), unlit(&mut nif));
    let mut material = vec![];
    words(&mut material, &[NONE, 0, NONE]);
    floats(&mut material, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 1.0, 1.0]);
    let material = nif.add("NiMaterialProperty", material);

    let data = corners(&mut nif, 1.0, true, None, LIST);
    let hidden = nif.shape("Hidden:0", 15, &[plain], data, NONE);
    let helper = nif.shape("Helper:0", 14, &[material], data, NONE);
    let bare = nif.shape("Bare:0", 14, &[], data, NONE);
    let marked = nif.shape("EditorMarker:0", 14, &[plain], data, NONE);
    let none = corners(&mut nif, 2.0, true, None, Corners::List(&[]));
    let rejected = nif.shape("Rejected:0", 14, &[plain], none, NONE);
    // Each of its triangles has a corner that is not there.
    let data = corners(&mut nif, 9.0, true, None, Corners::List(&[0, 1, 9, 8, 2, 1]));
    let gone = nif.shape("Gone:0", gone, &[plain], data, NONE);

    // Hidden, and shown by a controller of its own: the rig's alone.
    let data = corners(&mut nif, 3.0, true, Some(COLORS), LIST);
    let blink = nif.shape("Blink:0", 14, &[faded], data, NONE);
    let interpolator = nif.add("NiBoolInterpolator", [vec![1], NONE.to_le_bytes().to_vec()].concat());
    let blink_node = nif.blocks.len() as u32 + 1;
    let mut controller = vec![];
    words(&mut controller, &[NONE]);
    halves(&mut controller, &[8]); // active
    floats(&mut controller, &[1.0, 0.0, 0.0, 1.0]);
    words(&mut controller, &[blink_node, interpolator]);
    let controller = nif.add("NiVisController", controller);
    let node = nif.node("Blink", 15, controller, &[blink]);
    assert_eq!(nif.add("NiNode", node), blink_node);

    // Bound to the root, which the rig has: the rig draws a copy of its mesh.
    let mut skin = vec![];
    floats(&mut skin, &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    words(&mut skin, &[1]);
    skin.push(1);
    floats(&mut skin, &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    floats(&mut skin, &[0.0, 0.0, 0.0, 100.0]);
    halves(&mut skin, &[4]);
    for i in 0..4u16 {
        halves(&mut skin, &[i]);
        floats(&mut skin, &[1.0]);
    }
    let skin = nif.add("NiSkinData", skin);
    let mut instance = vec![];
    words(&mut instance, &[skin, NONE, 0, 1, 0]);
    let instance = nif.add("NiSkinInstance", instance);
    let data = corners(&mut nif, 4.0, true, None, LIST);
    let flag = nif.shape("Flag:0", 14, &[plain], data, instance);

    let data = corners(&mut nif, 5.0, true, Some(COLORS), LIST);
    let opaque = nif.shape("Opaque:0", 14, &[plain], data, NONE);
    let see_through = nif.shape("Faded:0", 14, &[faded], data, NONE);
    // A corner that is not there, and no normals.
    let data = corners(&mut nif, 6.0, false, None, Corners::List(&[0, 1, 2, 1, 9, 2, 1, 3, 2]));
    let torn = nif.shape("Torn:0", 14, &[plain], data, NONE);
    let data = corners(&mut nif, 7.0, false, None, Corners::Strip(&[0, 1, 2, 2, 1, 3]));
    let strip = nif.shape("Strip:0", 14, &[plain], data, NONE);
    nif.blocks[strip as usize].0 = "NiTriStrips";
    let data = corners(&mut nif, 8.0, true, Some(COLORS), LIST);
    let shine = nif.shape("Shine:0", 14, &[dark], data, NONE);

    let children = [hidden, helper, bare, marked, rejected, gone, blink_node, flag, opaque, see_through, torn, strip, shine];
    nif.blocks[0].1 = nif.node("Room", 14, NONE, &children);
    (made_nif::nif(&nif.blocks, &nif.strings), ["Blink:0", "Flag:0", "Opaque:0", "Faded:0", "Torn:0", "Strip:0", "Shine:0"])
}

/// A DXT1 texture of one block.
fn wall() -> Vec<u8> {
    let mut out = vec![0; 128 + 8];
    out[..4].copy_from_slice(b"DDS ");
    for (at, v) in [(4, 124u32), (8, 0x21007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4)] {
        out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    out[84..88].copy_from_slice(b"DXT1");
    out
}

struct Game {
    dir: PathBuf,
    world: World,
    state: SystemState<ModelCtx<'static>>,
}

impl Game {
    fn new(name: &str) -> Game {
        let dir = std::env::temp_dir().join(format!("nvf-read-again-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("meshes")).unwrap();
        std::fs::write(dir.join("meshes/room.nif"), room(15).0).unwrap();
        std::fs::write(dir.join("meshes/gone.nif"), room(14).0).unwrap();
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::write(dir.join("textures/wall.dds"), wall()).unwrap();
        let schema = fnv_nif::Schema::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
        let mut world = World::new();
        world.insert_resource(GameData::new(Arc::new(fnv_vfs::Vfs::open(&dir, &[]).unwrap()), Arc::new(schema)));
        world.init_resource::<ModelCache>();
        world.init_resource::<TextureCache>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<NifMaterial>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<SkinnedMeshInverseBindposes>>();
        let state = SystemState::new(&mut world);
        Game { dir, world, state }
    }

    fn ctx<R>(&mut self, f: impl FnOnce(&mut ModelCtx) -> R) -> R {
        f(&mut self.state.get_mut(&mut self.world).unwrap())
    }

    /// The model prepared here (`Stores`).
    fn stores(&mut self) -> Arc<PreparedModel> {
        self.ctx(|c| c.model(PATH)).expect("the model loads")
    }

    /// The model prepared on a worker (`Deferred`).
    fn deferred(&mut self) -> Arc<PreparedModel> {
        self.ctx(|c| {
            assert!(!c.models_ready([(PATH, &[][..])]) && c.cache.in_flight() == 1);
            let start = std::time::Instant::now();
            while c.take_fetched(0.0, None) == 0 {
                assert!(start.elapsed().as_secs() < 60, "the worker is done");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            c.model(PATH).expect("the model loads")
        })
    }

    fn mesh(&self, id: AssetId<Mesh>) -> &Mesh {
        self.world.resource::<Assets<Mesh>>().get(id).expect("the mesh is in the store")
    }

    fn bytes(&self) -> Vec<u8> {
        self.world.resource::<GameData>().read(PATH).unwrap()
    }

    fn read(&self, wanted: &[(u32, &str)]) -> Result<Vec<Geometry>, String> {
        geometry(&self.bytes(), &self.world.resource::<GameData>().schema, wanted)
    }

    fn sources(&self, wanted: &HashSet<AssetId<Mesh>>) -> HashMap<AssetId<Mesh>, Option<MeshSource>> {
        self.world.resource::<ModelCache>().sources(wanted)
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Every mesh of the model, from both of its lists: its handle's id, its block and its name.
fn meshes(m: &PreparedModel) -> Vec<(AssetId<Mesh>, u32, String)> {
    let shapes = m.shapes.iter().map(|s| (s.mesh.id(), s.block, s.name.clone().unwrap_or_default()));
    let rig = m.rig.iter().flat_map(|r| &r.shapes).map(|s| (s.mesh.id(), s.block, s.look.0.clone()));
    shapes.chain(rig).collect()
}

/// Positions, normals, texture coordinates, colours and indices, as bits.
type Bits = [Vec<u32>; 5];

fn bits<const N: usize>(v: &[[f32; N]]) -> Vec<u32> {
    v.as_flattened().iter().map(|x| x.to_bits()).collect()
}

fn drawn(m: &Mesh) -> Bits {
    let of = |attribute| match m.attribute(attribute) {
        Some(VertexAttributeValues::Float32x2(v)) => bits(v),
        Some(VertexAttributeValues::Float32x3(v)) => bits(v),
        Some(VertexAttributeValues::Float32x4(v)) => bits(v),
        None => Vec::new(),
        Some(_) => panic!("an attribute of another kind"),
    };
    let Some(Indices::U32(indices)) = m.indices() else { panic!("32 bit indices") };
    [of(Mesh::ATTRIBUTE_POSITION), of(Mesh::ATTRIBUTE_NORMAL), of(Mesh::ATTRIBUTE_UV_0), of(Mesh::ATTRIBUTE_COLOR), indices.clone()]
}

fn again(g: &Geometry) -> Bits {
    let m = &g.mesh;
    [bits(&m.positions), bits(&m.normals), bits(&m.uv0), bits(&m.colors), m.indices.clone()]
}

/// Each drawn shape's arrays by its block, bit for bit, from both sinks: also the rig's alone,
/// the skinned one's two copies, strips, made normals and both kinds of vertex colours. Fails
/// with a block counted among the drawn, with colours as stored or with `vertex_alpha` fixed.
#[test]
fn a_shapes_arrays_are_the_loaders() {
    let _turn = turn();
    let (_, names) = room(15);
    for (way, load) in [("Stores", Game::stores as fn(&mut Game) -> Arc<PreparedModel>), ("Deferred", Game::deferred)] {
        let mut game = Game::new(way);
        let model = load(&mut game);
        let drawn_names = |v: Vec<String>| v.into_iter().collect::<HashSet<_>>();
        let rig = model.rig.as_ref().expect("a rig");
        assert_eq!(drawn_names(rig.shapes.iter().map(|s| s.look.0.clone()).collect()), drawn_names(names.map(String::from).to_vec()), "{way}: the rig draws all seven");
        assert_eq!(drawn_names(model.shapes.iter().filter_map(|s| s.name.clone()).collect()), drawn_names(names[1..].iter().map(|n| n.to_string()).collect()), "{way}: all but the blinking one stand still");
        let flags: HashSet<_> = meshes(&model).into_iter().filter(|(.., n)| n == "Flag:0").map(|(id, ..)| id).collect();
        assert!(flags.len() == 2 && rig.shapes.iter().any(|s| s.skin.is_some()), "{way}: the skinned shape's two meshes");
        let all = meshes(&model);
        let wanted: Vec<(u32, &str)> = all.iter().map(|(_, block, name)| (*block, name.as_str())).collect();
        let read = game.read(&wanted).unwrap();
        assert_eq!(read.len(), all.len());
        let mut boxes = HashMap::new();
        for ((id, _, name), g) in all.iter().zip(&read) {
            assert_eq!(again(g), drawn(game.mesh(*id)), "{way} {name}");
            // The box that the renderer's taking of a mesh leaves behind.
            let mut mesh = game.mesh(*id).clone();
            mesh.take_gpu_data().unwrap();
            assert!(mesh.final_aabb.is_some() && g.same_box(mesh.final_aabb) && g.same_box(None), "{way} {name}");
            boxes.insert(name.as_str(), mesh.final_aabb);
        }
        assert!(!read[0].same_box(boxes["Strip:0"]), "{}", all[0].2);
        let of = |name: &str| &read[all.iter().position(|(.., n)| n == name).unwrap()].mesh;
        // The fixture is what it is to be, so that the comparison says something.
        let alphas = |name: &str| of(name).colors.iter().map(|c| c[3]).collect::<Vec<_>>();
        assert_eq!((alphas("Opaque:0"), alphas("Faded:0")), (vec![1.0; 4], vec![0.25, 0.75, 1.0, 0.0]));
        assert_eq!(of("Opaque:0").colors[..2], [[0.5, 1.0, 0.0, 1.0], [1.0, 0.0, 0.3, 1.0]]);
        assert_eq!(of("Torn:0").indices, [0, 1, 2, 1, 3, 2]);
        assert_eq!(of("Strip:0").indices, [0, 1, 2, 2, 3, 1]);
        assert!(of("Torn:0").normals.len() == 4 && of("Torn:0").colors.is_empty());
    }
}

/// Every variant of the file, with meshes of its own, is found with the same blocks, and a
/// mesh that two lists share once.
#[test]
fn every_variant_names_the_same_blocks() {
    let _turn = turn();
    let mut game = Game::new("variants");
    let swap = AltTexture { shape: "Opaque:0".into(), index: -1, texture_set: 1, textures: Default::default() };
    let clip = Clip::from_controllers(Vec::new(), Vec::new(), Vec::new());
    let models = game.ctx(|c| {
        [
            c.model(PATH),
            c.model_with_cpu(PATH),
            c.model_first_person(PATH),
            c.model_swapped(PATH, false, std::slice::from_ref(&swap)),
            c.model_swapped(PATH, true, std::slice::from_ref(&swap)),
            c.model_for_attachment(PATH),
            c.model_for_idle(PATH, 0x12345, &clip),
        ]
    });
    let all: Vec<_> = models.iter().map(|m| meshes(m.as_ref().expect("the model loads"))).collect();
    assert!(all.iter().all(|m| m.iter().map(|(_, block, name)| (block, name)).eq(all[0].iter().map(|(_, block, name)| (block, name)))), "the same shapes in the same order");
    let expected: HashMap<_, _> = all.iter().flatten().map(|(id, block, name)| (*id, Some(MeshSource { path: PATH.into(), block: *block, name: name.clone() }))).collect();
    assert_eq!(expected.len(), 7 * 8, "each variant has meshes of its own: seven shapes, one of them twice");
    assert_eq!(game.sources(&expected.keys().copied().collect()), expected);
    assert!(game.sources(&HashSet::new()).is_empty());
}

/// What is asked for and is not there is the file's error and names the block; a file that is
/// cut off or none is an error as well.
#[test]
fn what_is_not_there_is_an_error() {
    let _turn = turn();
    let mut game = Game::new("errors");
    // Where it shows, the loader takes the shape without triangles and draws it empty.
    let model = game.ctx(|c| c.model(r"meshes\gone.nif")).expect("the model loads");
    let gone = model.shapes.iter().find(|s| s.name.as_deref() == Some("Gone:0")).expect("taken");
    assert!(drawn(game.mesh(gone.mesh.id()))[4].is_empty() && game.read(&[(gone.block, "Gone:0")]).is_err());
    let (bytes, _) = room(15);
    let file = fnv_nif::NifFile::parse(&bytes, None).unwrap();
    let dec = fnv_nif::scene::Decoded::new(&file, &bytes, &game.world.resource::<GameData>().schema);
    let block = |name: &str| dec.blocks.iter().position(|b| b.as_ref().is_ok_and(|s| s.text("name") == Some(name))).unwrap() as u32;
    let opaque = (block("Opaque:0"), "Opaque:0");
    assert!(game.read(&[opaque]).is_ok_and(|g| g.len() == 1) && game.read(&[]).is_ok_and(|g| g.is_empty()));
    for (wanted, why) in [
        ((block("Blink"), "Blink"), "no shape"),
        ((opaque.0 - 1, "Opaque:0"), "no shape"),
        ((NONE, "Opaque:0"), "no shape"),
        ((opaque.0, "Faded:0"), "its name is Opaque:0"),
        ((block("Rejected:0"), "Rejected:0"), "nothing to draw"),
        ((block("Gone:0"), "Gone:0"), "nothing to draw"),
    ] {
        let e = game.read(&[opaque, wanted]).unwrap_err();
        assert_eq!(e, format!("block {} ({}): {why}", wanted.0, wanted.1));
    }
    let schema = &game.world.resource::<GameData>().schema;
    for cut in [0, 10, 60, bytes.len() / 2] {
        assert!(geometry(&bytes[..cut], schema, &[opaque]).is_err(), "cut at {cut}");
    }
    assert!(geometry(&[0xA5; 4096], schema, &[opaque]).is_err());
}

/// The run's counters that loading a model moves.
fn counters() -> [String; 6] {
    use nvf_assets::{authored, load_times, nif_spawn, tangents};
    [authored::stats().line(), authored::stats().game_line(1.0), load_times::summary(), nif_spawn::vertex_alpha_blended_summary(), nif_spawn::blend_opaque_summary(), tangents::summary()]
}

/// Everything in the stores with what it holds, the bind poses by their ids.
fn stores(w: &World) -> String {
    let mut others: Vec<String> = w.resource::<Assets<SkinnedMeshInverseBindposes>>().ids().map(|id| format!("{id:?}")).collect();
    others.extend(w.resource::<Assets<NifMaterial>>().iter().map(|(id, m)| format!("{id:?} {:?} {:?}", m.base, m.extension)));
    others.sort();
    let mut meshes: Vec<String> = w.resource::<Assets<Mesh>>().iter().map(|(id, m)| format!("{id:?} {:?} {:?} {:?}", m.asset_usage, m.final_aabb, drawn(m))).collect();
    meshes.sort();
    let images: Vec<String> = w.resource::<Assets<Image>>().iter().map(|(id, i)| format!("{id:?} {:?} {:?} {:?}", i.asset_usage, i.texture_descriptor, i.data)).collect();
    let held = (w.resource::<ModelCache>().len(), w.resource::<TextureCache>().len(), w.resource::<TextureCache>().generation(), &w.resource::<TextureCache>().failures);
    format!("{meshes:?} {images:?} {others:?} {held:?}")
}

/// What this thread logs while `f` runs.
fn logged(f: impl FnOnce()) -> String {
    use bevy::log::tracing_subscriber::{fmt, util::SubscriberInitExt};
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let lines = Lines::default();
    let to = lines.clone();
    let guard = fmt().with_writer(move || to.clone()).with_max_level(bevy::log::Level::TRACE).finish().set_default();
    f();
    drop(guard);
    let text = lines.0.lock().unwrap().clone();
    String::from_utf8_lossy(&text).into_owned()
}

/// Reading again leaves no trace. Preparing the model again would: a file's times, a blended
/// shape, lines in the log and new handles; the test's own load shows that these are seen.
#[test]
fn reading_again_changes_nothing() {
    let _turn = turn();
    let mut game = Game::new("nothing");
    let fresh = counters();
    let mut model = None;
    let log = logged(|| model = Some(game.stores()));
    let model = model.unwrap();
    let loaded = counters();
    assert!(fresh[2] != loaded[2] && fresh[3] != loaded[3], "{loaded:?}");
    assert!(log.contains("controllers of their own"), "{log}");
    let wall = game.ctx(|c| c.textures.get("wall.dds", true, &c.data, &mut c.images, None)).expect("the texture loads");
    let wall_drawn = Drawn::of(game.world.resource::<Assets<Image>>().get(&wall).unwrap()).unwrap();
    let (bytes, data) = (game.bytes(), game.world.resource::<GameData>().clone());
    let before = stores(&game.world);
    let all = meshes(&model);
    let wanted: Vec<(u32, &str)> = all.iter().map(|(_, block, name)| (*block, name.as_str())).collect();
    let log = logged(|| {
        geometry(&bytes, &data.schema, &wanted).unwrap();
        assert!(geometry(&bytes, &data.schema, &[(0, "Room")]).is_err());
        let sources = game.world.resource::<TextureCache>().sources(&HashSet::from([wall.id()]));
        let source = sources[&wall.id()].as_ref().unwrap();
        assert_eq!(levels::read(&data, source, &wall_drawn, true).unwrap().levels.len(), 1);
        assert!(levels::read(&data, &ImageSource { path: "none.dds".into(), ..source.clone() }, &wall_drawn, true).is_err());
        assert_eq!(game.sources(&all.iter().map(|(id, ..)| *id).collect()).len(), 8);
    });
    assert_eq!(log, "");
    assert_eq!(counters(), loaded);
    assert_eq!(stores(&game.world), before);
}
