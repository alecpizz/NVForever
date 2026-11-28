//! Compiled requests through real cached ModelCtx and the production graph/mirror.
//! Authored files only. ScriptState acquisition and GPU drawing are not exercised.
use super::*;
use crate::nif_attachments::{self, Owned, State};
use bevy::{camera::visibility::RenderLayers, ecs::system::{RunSystemOnce, SystemState}};
use fnv_game::{game::RunCtx, Game, GameConfig};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormStore, RefTable};
use nvf_assets::model::{ModelCtx, PreparedModel};
use std::collections::HashSet;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use crate::authored_test_records as common;
const REFERENCE: FormId = FormId(0x401);
const OTHER: FormId = FormId(0x402);
const IDENTITY: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Bytes(Vec<u8>);
impl Bytes {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn f(&mut self, values: &[f32]) -> &mut Self {
        for v in values {
            self.0.extend(v.to_le_bytes());
        }
        self
    }
    fn text(&mut self, s: &str) -> &mut Self {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
        self
    }
    fn export(&mut self, s: &str) -> &mut Self {
        self.u8(s.len() as u8 + 1);
        self.0.extend(s.as_bytes());
        self.u8(0)
    }
}

fn av(w: &mut Bytes, name: u32, position: [f32; 3], properties: &[i32]) {
    w.u32(name)
        .u32(0)
        .i32(-1)
        .u32(14)
        .f(&position)
        .f(&IDENTITY)
        .f(&[1.]);
    w.u32(properties.len() as u32);
    for &p in properties {
        w.i32(p);
    }
    w.i32(-1);
}
fn node(name: u32, position: [f32; 3], children: &[i32]) -> Vec<u8> {
    let mut w = Bytes::default();
    av(&mut w, name, position, &[]);
    w.u32(children.len() as u32);
    for &child in children {
        w.i32(child);
    }
    w.u32(0);
    w.0
}
fn shape(name: u32, position: [f32; 3], data: i32, material: i32, shader: i32) -> Vec<u8> {
    let mut w = Bytes::default();
    av(&mut w, name, position, &[material, shader]);
    w.i32(data).i32(-1).u32(0).i32(-1).u8(0);
    w.0
}
fn geometry(width: f32) -> Vec<u8> {
    let mut w = Bytes::default();
    w.i32(0).u16(4).u8(0).u8(0).u8(1);
    w.f(&[0., 0., 0., width, 0., 0., 0., 1., 0., width, 1., 0.]);
    w.u16(1).u8(0).f(&[0., 0., 0., 4.]).u8(0);
    w.f(&[0., 0., 1., 0., 0., 1., 1., 1.]);
    w.u16(0)
        .i32(-1)
        .u16(2)
        .u16(1)
        .u16(4)
        .u8(1)
        .u16(0)
        .u16(1)
        .u16(2)
        .u16(3);
    w.0
}
fn material(gloss: f32) -> Vec<u8> {
    let mut w = Bytes::default();
    w.u32(u32::MAX).u32(0).i32(-1);
    w.f(&[1., 0., 0., 0., 0., 0., gloss, 1., 1.]);
    w.0
}
// Authored BSShaderPPLightingProperty, matching the existing nif_spawn test builder.
// Material-only shapes are helper geometry; this shader makes the fixture drawable.
fn lit_shader() -> Vec<u8> {
    let mut w = Bytes::default();
    w.u32(u32::MAX).u32(0).i32(-1).u16(1);
    w.u32(1).u32(0).u32(0).f(&[1.]);
    w.u32(3).i32(-1).f(&[0.]).u32(0).f(&[4., 1.]);
    w.0
}
fn nif(blocks: &[(&str, Vec<u8>)], strings: &[&str]) -> Vec<u8> {
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    let mut w = Bytes(b"Gamebryo File Format, Version 20.2.0.7\n".to_vec());
    w.u32(0x1402_0007)
        .u8(1)
        .u32(11)
        .u32(blocks.len() as u32)
        .u32(34);
    w.export("owned")
        .export("fixture")
        .export("synthetic")
        .u16(types.len() as u16);
    for name in &types {
        w.text(name);
    }
    for (name, _) in blocks {
        w.u16(types.iter().position(|n| n == name).unwrap() as u16);
    }
    for (_, bytes) in blocks {
        w.u32(bytes.len() as u32);
    }
    w.u32(strings.len() as u32)
        .u32(strings.iter().map(|s| s.len()).max().unwrap_or(0) as u32);
    for s in strings {
        w.text(s);
    }
    w.u32(0);
    for (_, bytes) in blocks {
        w.0.extend(bytes);
    }
    w.u32(1).i32(0);
    w.0
}
fn flagged_shape(name: u32, position: [f32; 3], data: i32, material: i32, shader: i32, flags: u32) -> Vec<u8> {
    let mut bytes = shape(name, position, data, material, shader);
    bytes[12..16].copy_from_slice(&flags.to_le_bytes());
    bytes
}
fn duplicate_file(label: &str) -> Vec<u8> {
    nif(
        &[
            ("BSFadeNode", node(0, [3., 4., 5.], &[1, 2])),
            ("NiNode", node(1, [1., 0., 0.], &[3])),
            ("NiNode", node(2, [0., 2., 0.], &[4])),
            ("NiTriStrips", shape(3, [2., 0., 0.], 5, 7, 9)),
            ("NiTriStrips", flagged_shape(3, [0., 3., 0.], 6, 8, 9, 46)),
            ("NiTriStripsData", geometry(1.)),
            ("NiTriStripsData", geometry(2.)),
            ("NiMaterialProperty", material(11.)),
            ("NiMaterialProperty", material(22.)),
            ("BSShaderPPLightingProperty", lit_shader()),
        ],
        &["OwnedNifRoot", "BranchA", "BranchB", label],
    )
}
fn single_file(unnamed: bool) -> Vec<u8> {
    nif(
        &[
            ("BSFadeNode", node(0, [3., 4., 5.], &[1])),
            ("NiNode", node(1, [1., 0., 0.], &[2])),
            (
                "NiTriStrips",
                shape(if unnamed { u32::MAX } else { 2 }, [2., 0., 0.], 3, 4, 5),
            ),
            ("NiTriStripsData", geometry(1.)),
            ("NiMaterialProperty", material(11.)),
            ("BSShaderPPLightingProperty", lit_shader()),
        ],
        &["OwnedNifRoot", "BranchA", "UniqueShape"],
    )
}

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nvf-static-identity-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("meshes")).unwrap();
        std::fs::create_dir(root.join("meshes/owned")).unwrap();
        for (name, bytes) in [
            ("duplicate.nif", duplicate_file("Shared")),
            ("unique.nif", single_file(false)),
            ("unnamed.nif", single_file(true)),
            ("multiple.nif", multiple_file()),
        ] {
            std::fs::write(root.join("meshes/owned").join(name), bytes).unwrap();
        }
        Self(root)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        for name in ["duplicate.nif", "unique.nif", "unnamed.nif", "multiple.nif"] {
            let _ = std::fs::remove_file(self.0.join("meshes/owned").join(name));
        }
        for dir in [
            self.0.join("meshes/owned"),
            self.0.join("meshes"),
            self.0.clone(),
        ] {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

struct Source {
    data: fnv_game::GameData,
    forms: FormStore,
    refs: RefTable,
}
fn source() -> Source {
    use common::*;
    let references = [
        record(
            b"REFR",
            0,
            REFERENCE.0,
            &[
                sub(b"EDID", z("OwnedReference")),
                sub(b"NAME", u32s(&[0x301])),
                sub(b"DATA", vec![0; 24]),
            ],
        ),
        record(
            b"REFR",
            0,
            OTHER.0,
            &[
                sub(b"EDID", z("OwnedOtherReference")),
                sub(b"NAME", u32s(&[0x301])),
                sub(b"DATA", vec![0; 24]),
            ],
        ),
    ]
    .concat();
    let cell = [
        record(
            b"CELL",
            0,
            0x900,
            &[sub(b"EDID", z("OwnedCell")), sub(b"DATA", vec![1])],
        ),
        group(
            0x900u32.to_le_bytes(),
            6,
            &group(0x900u32.to_le_bytes(), 9, &references),
        ),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(
            b"STAT",
            &record(b"STAT", 0, 0x301, &[sub(b"EDID", z("OwnedBase"))]),
        ),
        top(
            b"CELL",
            &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)),
        ),
    ]
    .concat();
    let plugins = [fnv_esm::Plugin::parse(&bytes, "OwnedAttachment.esm".into()).unwrap()];
    let lo = fnv_esm::LoadOrder::new(&plugins).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
    let catalogue = root.join("script/functions.json");
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let raw = serde_json::from_slice(&std::fs::read(&catalogue).unwrap()).unwrap();
    let data = fnv_game::GameData::build(
        &lo,
        &schema,
        FunctionTable::load(&catalogue).unwrap(),
        &raw,
        &refs,
    );
    Source { data, forms, refs }
}

fn multiple_file() -> Vec<u8> {
    let mut bytes = nif(
        &[
            ("NiNode", node(0, [3., 4., 5.], &[2])),
            ("NiNode", node(1, [30., 40., 50.], &[3])),
            ("NiTriStrips", shape(2, [2., 0., 0.], 4, 6, 8)),
            ("NiTriStrips", shape(u32::MAX, [0., 3., 0.], 5, 7, 8)),
            ("NiTriStripsData", geometry(1.)),
            ("NiTriStripsData", geometry(2.)),
            ("NiMaterialProperty", material(11.)),
            ("NiMaterialProperty", material(22.)),
            ("BSShaderPPLightingProperty", lit_shader()),
        ],
        &["RootA", "RootB", "ShapeA"],
    );
    bytes.truncate(bytes.len() - 8);
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(0i32.to_le_bytes());
    bytes.extend(1i32.to_le_bytes());
    bytes
}
#[derive(Resource)]
struct TestGame(Game<'static>);
#[derive(Resource, Default)]
struct TestMemory(Memory);
#[allow(clippy::too_many_arguments)]
fn produce(
    mut game: ResMut<TestGame>,
    mut state: ResMut<State>,
    commands: Commands,
    models: Option<ModelCtx>,
    rigs: Rigs,
    visuals: Visuals,
    nodes: NodeQuery,
    parents: Query<&ChildOf>,
    layers: Query<&RenderLayers>,
    owned: Owned,
) {
    nif_attachments::produce_game(
        Some(&mut game.0),
        &mut state,
        commands,
        models,
        rigs,
        visuals,
        nodes,
        parents,
        layers,
        owned,
    );
}
#[allow(clippy::too_many_arguments)]
fn publish(
    mut game: ResMut<TestGame>,
    rigs: Rigs,
    visuals: Visuals,
    parents: Query<&ChildOf>,
    nodes: NodeQuery,
    commands: Commands,
    mut memory: ResMut<TestMemory>,
    materials: Materials,
    shapes: Shapes,
    extra: Extra,
    dropped: Dropped,
) {
    mirror_game(
        &mut game.0,
        rigs,
        visuals,
        parents,
        nodes,
        commands,
        &mut memory.0,
        materials,
        shapes,
        extra,
        dropped,
    );
}
fn fixture(cold: bool) -> (App, Files, Entity, Entity) {
    let files = Files::new();
    let source = Box::leak(Box::new(source()));
    let game = Game::new(
        &source.data,
        fnv_sim::World::new(&source.forms, &source.refs, Default::default()),
        GameConfig {
            strict: true,
            ..default()
        },
    );
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif");
    let data = GameData::open_for_plugins(&files.0, &[] as &[&str], &schema).unwrap();
    let mut app = App::new();
    app.insert_resource(data)
        .insert_resource(TestGame(game))
        .init_resource::<TestMemory>()
        .init_resource::<nvf_assets::model::ModelCache>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<NifMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .init_resource::<nvf_assets::TextureCache>();
    nif_attachments::install(&mut app);
    app.add_systems(Update, (produce, publish).chain());
    let (root, socket) = host(app.world_mut(), REFERENCE, "Socket");
    host(app.world_mut(), OTHER, "Socket");
    if !cold {
        query(&mut app, "OwnedReference.GetChildBlocks \"\"");
        query(&mut app, "OwnedOtherReference.GetChildBlocks \"\"");
        app.update();
    }
    (app, files, root, socket)
}
fn host(world: &mut World, reference: FormId, socket: &str) -> (Entity, Entity) {
    let root = world
        .spawn((
            Name::new("OwnerRoot"),
            Transform::from_xyz(100., 200., 300.),
            Visibility::Inherited,
            nvf_play::RefVisual(reference.0),
        ))
        .id();
    let socket = world
        .spawn((
            Name::new(socket.to_string()),
            Transform::from_xyz(10., 0., 0.)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::splat(2.)),
            Visibility::Inherited,
            ChildOf(root),
        ))
        .id();
    (root, socket)
}
fn query(app: &mut App, expression: &str) -> XVal {
    let code = format!("Begin Function {{ }}\nSetFunctionValue ({expression})\nEnd\n");
    app.world_mut()
        .resource_mut::<TestGame>()
        .0
        .run_query(&code, RunCtx::default())
        .unwrap_or_else(|e| panic!("{code}: {e}"))
}
fn attach(app: &mut App, receiver: &str, mode: u8, target: &str, file: &str) -> XVal {
    query(
        app,
        &format!("AttachModel {receiver} {mode} \"{target}|*_Owned*owned/{file}\""),
    )
}
fn prepare(app: &mut App, file: &str) -> Arc<PreparedModel> {
    let mut state = SystemState::<ModelCtx>::new(app.world_mut());
    let model = state
        .get_mut(app.world_mut())
        .unwrap()
        .model_for_attachment(&format!("meshes/owned/{file}"))
        .expect("authored NIF prepares through real cache");
    state.apply(app.world_mut());
    model
}
fn members(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, With<nif_attachments::AttachedNode>>()
        .iter(app.world())
        .collect()
}
fn meshes(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, (With<nif_attachments::AttachedNode>, With<Mesh3d>)>()
        .iter(app.world())
        .collect()
}
fn scene(app: &mut App, name: &str) -> SceneNode {
    app.world_mut()
        .resource_mut::<TestGame>()
        .0
        .scene
        .node_now(REFERENCE, name)
        .unwrap()
}
fn composed(world: &World, mut entity: Entity) -> bevy::math::Affine3A {
    let mut t = bevy::math::Affine3A::IDENTITY;
    let mut seen = HashSet::new();
    loop {
        assert!(seen.insert(entity));
        t = world.get::<Transform>(entity).unwrap().compute_affine() * t;
        let Some(up) = world.get::<ChildOf>(entity) else {
            break;
        };
        entity = up.parent();
    }
    t
}

#[test]
fn compiled_attach_draws_distinct_duplicate_shapes_and_publishes_actual_identities() {
    let (mut app, _files, root, _) = fixture(false);
    let model = prepare(&mut app, "duplicate.nif");
    assert!(model.rig.is_none());
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(
        attach(&mut app, "OwnedReference", 3, "Socket", "duplicate.nif"),
        XVal::Num(1.)
    );
    app.update();
    let actual = meshes(&mut app);
    assert_eq!(actual.len(), 2);
    for (name, index, position) in [
        ("Shared_Owned", 0, Vec3::new(102., 212., 310.)),
        ("Shared_Owned:0", 1, Vec3::new(92., 206., 310.)),
    ] {
        let published = scene(&mut app, name);
        let identity = published.info.unwrap().identity.unwrap();
        let entity = actual
            .iter()
            .copied()
            .find(|e| e.to_bits() == identity.node)
            .unwrap();
        assert_eq!(identity.model, root.to_bits());
        assert_eq!(published.flags, if index == 0 { 14 } else { 46 });
        assert_eq!(
            published.material.unwrap()[6],
            if index == 0 { 11. } else { 22. }
        );
        assert_eq!(
            app.world().get::<Mesh3d>(entity).unwrap().0,
            model.shapes[index].mesh
        );
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<NifMaterial>>(entity)
                .unwrap()
                .0,
            model.shapes[index].material
        );
        assert!(
            (composed(app.world(), entity).transform_point3(Vec3::ZERO) - position).length() < 1e-4
        );
    }
    query(&mut app, "OwnedReference.SetNifScale \"Shared_Owned:0\" 3");
    query(
        &mut app,
        "OwnedReference.SetMatrPropVal \"Shared_Owned:0\" 6 77",
    );
    query(
        &mut app,
        "OwnedReference.SetMatrPropVal \"Shared_Owned:0\" 0 0.25",
    );
    app.update();
    assert_eq!(scene(&mut app, "Shared_Owned").scale, 1.);
    let edited = scene(&mut app, "Shared_Owned:0");
    assert_eq!(edited.scale, 3.);
    assert_eq!(edited.material.unwrap()[6], 77.);
    assert_eq!(edited.material.unwrap()[0], 0.25);
    let entity = actual
        .into_iter()
        .find(|e| e.to_bits() == edited.info.as_ref().unwrap().identity.unwrap().node)
        .unwrap();
    let handle = &app
        .world()
        .get::<MeshMaterial3d<NifMaterial>>(entity)
        .unwrap()
        .0;
    assert_ne!(*handle, model.shapes[1].material);
    assert_eq!(
        app.world()
            .resource::<Assets<NifMaterial>>()
            .get(handle)
            .unwrap()
            .base
            .perceptual_roughness,
        nvf_assets::nif_spawn::gloss_roughness(77.)
    );
    assert_eq!(scene(&mut app, "Shared_Owned").material.unwrap()[6], 11.);
    assert_eq!(
        app.world()
            .resource::<Assets<NifMaterial>>()
            .get(&model.shapes[1].material)
            .unwrap()
            .base
            .perceptual_roughness,
        nvf_assets::nif_spawn::gloss_roughness(22.)
    );
    let before: HashSet<_> = members(&mut app).into_iter().collect();
    attach(&mut app, "OwnedReference", 3, "Socket", "duplicate.nif");
    app.update();
    assert_eq!(
        members(&mut app).into_iter().collect::<HashSet<_>>(),
        before
    );
    attach(
        &mut app,
        "OwnedOtherReference",
        3,
        "Socket",
        "duplicate.nif",
    );
    app.update();
    assert_eq!(meshes(&mut app).len(), 4);
    assert!(Arc::ptr_eq(&model, &prepare(&mut app, "duplicate.nif")));
}
#[test]
fn unnamed_and_multiple_roots_keep_every_drawable_and_detach_every_owned_root() {
    for file in ["unnamed.nif", "multiple.nif"] {
        let (mut app, _files, _, socket) = fixture(false);
        let model = prepare(&mut app, file);
        assert_eq!(
            model.shapes.len(),
            if file == "multiple.nif" { 2 } else { 1 }
        );
        attach(&mut app, "OwnedReference", 3, "Socket", file);
        app.update();
        assert_eq!(meshes(&mut app).len(), model.shapes.len());
        assert_eq!(
            app.world().get::<Children>(socket).unwrap().len(),
            if file == "multiple.nif" { 2 } else { 1 }
        );
        for entity in meshes(&mut app) {
            assert!(model
                .shapes
                .iter()
                .any(|s| s.mesh == app.world().get::<Mesh3d>(entity).unwrap().0));
        }
        attach(&mut app, "OwnedReference", 2, "Socket", file);
        app.update();
        assert!(members(&mut app).is_empty());
        assert!(app.world().get::<Transform>(socket).is_some());
    }
}
#[test]
fn cold_parent_waits_but_missing_file_never_publishes_a_fake_graph() {
    let (mut app, _files, root, socket) = fixture(true);
    app.world_mut().despawn(socket);
    assert_eq!(
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"),
        XVal::Num(0.)
    );
    app.update();
    assert!(members(&mut app).is_empty());
    let socket = app
        .world_mut()
        .spawn((Name::new("Socket"), Transform::IDENTITY, ChildOf(root)))
        .id();
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    let actual = members(&mut app);
    assert!(actual
        .iter()
        .any(|e| app.world().get::<ChildOf>(*e).unwrap().parent() == socket));
    let before: HashSet<_> = actual.into_iter().collect();
    attach(&mut app, "OwnedReference", 3, "Socket", "absent.nif");
    app.update();
    assert_eq!(
        members(&mut app).into_iter().collect::<HashSet<_>>(),
        before
    );
    assert_eq!(
        scene(&mut app, "UniqueShape_Owned").material.unwrap()[6],
        11.
    );
}
#[test]
fn mode_two_uses_instance_ownership_after_rename_and_preserves_foreign_children() {
    let (mut app, _files, root, socket) = fixture(false);
    let second = app
        .world_mut()
        .spawn((Name::new("OtherSocket"), Transform::IDENTITY, ChildOf(root)))
        .id();
    app.update();
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    attach(&mut app, "OwnedReference", 3, "OtherSocket", "unique.nif");
    app.update();
    let owned = members(&mut app);
    let attached_root = owned
        .iter()
        .copied()
        .find(|e| app.world().get::<ChildOf>(*e).unwrap().parent() == socket)
        .unwrap();
    let neighbor: HashSet<_> = owned
        .iter()
        .copied()
        .filter(|e| {
            let mut at = *e;
            for _ in 0..16 {
                if at == second {
                    return true;
                };
                let Some(up) = app.world().get::<ChildOf>(at) else {
                    return false;
                };
                at = up.parent();
            }
            false
        })
        .collect();
    assert_eq!(neighbor.len(), 3);
    query(
        &mut app,
        "OwnedReference.RenameNifBlock \"OwnedNifRoot_Owned\" \"RenamedOwnedRoot\"",
    );
    app.update();
    assert_eq!(
        app.world().get::<Name>(attached_root).unwrap().as_str(),
        "RenamedOwnedRoot"
    );
    let foreign = app
        .world_mut()
        .spawn((
            Name::new("OwnedNifRoot_Owned"),
            Transform::IDENTITY,
            ChildOf(attached_root),
        ))
        .id();
    attach(&mut app, "OwnedReference", 2, "Socket", "unique.nif");
    app.update();
    assert_eq!(
        members(&mut app).into_iter().collect::<HashSet<_>>(),
        neighbor
    );
    assert!(app.world().get::<Transform>(foreign).is_some());
    assert!(app.world().get::<ChildOf>(foreign).is_none());
    assert!(app.world().get::<Transform>(socket).is_some());
}
#[test]
fn synchronous_parent_retirement_expires_nodes_and_holds_without_namesake_rebinding() {
    for part in ["parent", "root", "child"] {
        let (mut app, _files, root, socket) = fixture(false);
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
        app.update();
        let old = meshes(&mut app)[0];
        query(
            &mut app,
            "OwnedReference.SetNifScale \"UniqueShape_Owned\" 7",
        );
        app.update();
        assert_eq!(
            app.world().get::<Transform>(old).unwrap().scale,
            Vec3::splat(7.)
        );
        let removed = match part {
            "root" => root,
            "child" => old,
            _ => socket,
        };
        let transform = *app.world().get::<Transform>(removed).unwrap();
        app.world_mut()
            .entity_mut(removed)
            .remove::<Transform>()
            .insert(transform); // no update between
        app.update();
        assert!(members(&mut app).is_empty());
        assert!(app.world().get::<Transform>(old).is_none());
        app.update();
        assert!(members(&mut app).is_empty());
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
        app.update();
        let new = meshes(&mut app)[0];
        assert_ne!(new, old);
        assert_eq!(app.world().get::<Transform>(new).unwrap().scale, Vec3::ONE);
    }
}
#[test]
fn replacement_replays_structure_but_stale_capture_cannot_create_in_replacement() {
    let (mut app, _files, root, _) = fixture(false);
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.world_mut()
        .entity_mut(root)
        .remove::<nvf_play::RefVisual>();
    let (replacement, _) = host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    assert!(
        members(&mut app).is_empty(),
        "the captured old parent must not bind by name"
    );
    app.world_mut()
        .entity_mut(replacement)
        .remove::<nvf_play::RefVisual>();
    let (replacement, _) = host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    assert!(
        members(&mut app).is_empty(),
        "a second replacement does not revive the stale request"
    );
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    let old = meshes(&mut app)[0];
    query(
        &mut app,
        "OwnedReference.SetNifScale \"UniqueShape_Owned\" 7",
    );
    app.update();
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    assert_eq!(
        meshes(&mut app),
        vec![old],
        "a repeated live registration is still one instance"
    );
    app.world_mut()
        .entity_mut(replacement)
        .remove::<nvf_play::RefVisual>();
    let (new_root, _) = host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    let new = meshes(&mut app)[0];
    assert_ne!(old, new);
    assert!(app.world().get::<Transform>(old).is_none());
    assert_eq!(app.world().get::<Transform>(new).unwrap().scale, Vec3::ONE);
    assert_eq!(
        scene(&mut app, "UniqueShape_Owned")
            .info
            .unwrap()
            .identity
            .unwrap()
            .model,
        new_root.to_bits()
    );
}
#[test]
fn deferred_add_and_remove_have_separate_existing_and_future_model_behavior() {
    let (mut app, _files, root, _) = fixture(false);
    attach(&mut app, "OwnedReference", 1, "Socket", "unique.nif");
    app.update();
    assert!(members(&mut app).is_empty());
    app.world_mut()
        .entity_mut(root)
        .remove::<nvf_play::RefVisual>();
    let (next, _) = host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    let old: HashSet<_> = members(&mut app).into_iter().collect();
    attach(&mut app, "OwnedReference", 0, "Socket", "unique.nif");
    app.update();
    assert_eq!(members(&mut app).into_iter().collect::<HashSet<_>>(), old);
    app.world_mut()
        .entity_mut(next)
        .remove::<nvf_play::RefVisual>();
    host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    assert!(members(&mut app).is_empty());
}
#[test]
fn cached_source_graph_stays_coherent_after_file_rewrite_and_invalid_mapping_is_atomic() {
    let (mut app, files, _, _) = fixture(false);
    let cached = prepare(&mut app, "duplicate.nif");
    std::fs::write(
        files.0.join("meshes/owned/duplicate.nif"),
        single_file(false),
    )
    .unwrap();
    attach(&mut app, "OwnedReference", 3, "Socket", "duplicate.nif");
    app.update();
    assert_eq!(meshes(&mut app).len(), 2);
    assert_eq!(scene(&mut app, "Shared_Owned:0").material.unwrap()[6], 22.);
    assert!(Arc::ptr_eq(&cached, &prepare(&mut app, "duplicate.nif")));
    let mut malformed = (*cached).clone();
    malformed.objects[0].parent = Some(0);
    assert!(!nif_attachments::valid(&malformed));
    malformed = (*cached).clone();
    malformed.objects[1].block = malformed.objects[0].block;
    assert!(!nif_attachments::valid(&malformed));
    malformed = (*cached).clone();
    malformed
        .objects
        .iter_mut()
        .find(|o| o.shape.is_some())
        .unwrap()
        .shape = Some(usize::MAX);
    assert!(!nif_attachments::valid(&malformed));
    assert_eq!(meshes(&mut app).len(), 2);
}

#[test]
fn cold_reference_first_birth_and_session_replacement_do_not_reuse_old_owners() {
    let (mut app, _files, root, _) = fixture(true);
    app.world_mut()
        .entity_mut(root)
        .remove::<nvf_play::RefVisual>();
    assert_eq!(
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"),
        XVal::Num(0.)
    );
    app.update();
    assert!(members(&mut app).is_empty());
    let (current, _) = host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    let old = members(&mut app);
    let game = &app.world().resource::<TestGame>().0;
    let new = Game::new(
        game.data,
        fnv_sim::World::new(game.world.forms, game.world.placed, Default::default()),
        Default::default(),
    );
    app.insert_resource(TestGame(new));
    app.update();
    assert!(members(&mut app).is_empty());
    assert!(old
        .iter()
        .all(|e| app.world().get::<Transform>(*e).is_none()));
    assert!(app.world().get::<Transform>(current).is_some());
    query(&mut app, "OwnedReference.GetChildBlocks \"\"");
    app.update();
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    assert!(!old.contains(&meshes(&mut app)[0]));
}

#[test]
fn parent_namesake_and_missing_graph_retry_do_not_steal_an_old_registration() {
    let (mut app, _files, root, socket) = fixture(false);
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    app.world_mut().despawn(socket);
    let replacement = app
        .world_mut()
        .spawn((Name::new("Socket"), Transform::IDENTITY, ChildOf(root)))
        .id();
    app.update();
    assert!(members(&mut app).is_empty());
    app.update();
    assert!(members(&mut app).is_empty());
    assert!(app.world().get::<Children>(replacement).is_none());
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    assert_eq!(app.world().get::<Children>(replacement).unwrap().len(), 1);
}

#[test]
fn indexed_rig_spawn_preserves_group_rig_and_unrelated_prepared_materials() {
    use nvf_assets::model::{Rig, RigNode, RigShape};
    let (mut app, _files, _, socket) = fixture(false);
    let cached = prepare(&mut app, "duplicate.nif");
    let mut authored = (*cached).clone();
    for (i, object) in authored.objects.iter_mut().enumerate() {
        object.rig_node = Some(i);
    }
    let nodes = authored
        .objects
        .iter()
        .map(|o| RigNode {
            name: o.name.clone(),
            parent: o.parent,
            local: o.local,
            hidden: false,
            billboard: None,
        })
        .collect();
    let shapes = authored
        .objects
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            o.shape.map(|s| {
                let s = &authored.shapes[s];
                RigShape {
                    node: i,
                    mesh: s.mesh.clone(),
                    material: s.material.clone(),
                    skin: None,
                    look: (o.name.clone().unwrap_or_default(), [0.; 3], 1., 1.),
                    refraction: 0.,
                    animated: true,
                    morph: None,
                    block: 0,
                }
            })
        })
        .collect();
    let mut attached_clip =
        nvf_assets::anim_play::Clip::from_controllers(Vec::new(), Vec::new(), Vec::new());
    attached_clip.name = "AttachmentIdle".into();
    authored.rig = Some(Rig {
        nodes,
        shapes,
        idle: Some(attached_clip),
        door: None,
        groups: Vec::new(),
        effect: None,
    });
    assert!(nif_attachments::valid(&authored));
    let neighbor = app
        .world_mut()
        .spawn((
            Mesh3d(cached.shapes[0].mesh.clone()),
            MeshMaterial3d(cached.shapes[0].material.clone()),
            Transform::IDENTITY,
        ))
        .id();
    fn spawn(In((model, parent)): In<(Arc<PreparedModel>, Entity)>, mut commands: Commands) {
        nif_attachments::spawn_model(
            &mut commands,
            &model,
            parent,
            "authored-rig",
            "_Rig",
            RenderLayers::layer(3),
            700,
        );
    }
    app.world_mut()
        .run_system_cached_with(spawn, (Arc::new(authored), socket))
        .unwrap();
    let root = members(&mut app)
        .into_iter()
        .find(|e| app.world().get::<ChildOf>(*e).unwrap().parent() == socket)
        .unwrap();
    assert!(app
        .world()
        .get::<crate::object_anims::GroupRig>(root)
        .is_none());
    assert_eq!(
        app.world()
            .get::<nvf_assets::anim_play::Playing>(root)
            .unwrap()
            .name,
        "AttachmentIdle"
    );
    let branch = members(&mut app)
        .into_iter()
        .find(|e| {
            app.world()
                .get::<Name>(*e)
                .is_some_and(|n| n.as_str() == "BranchB_Rig")
        })
        .unwrap();
    assert_eq!(app.world().get::<ChildOf>(branch).unwrap().parent(), root);
    let mut host_clip =
        nvf_assets::anim_play::Clip::from_controllers(Vec::new(), Vec::new(), Vec::new());
    host_clip.name = "HostSequence".into();
    let host_group = app
        .world_mut()
        .spawn((
            crate::object_anims::GroupRig::new(
                REFERENCE.0,
                cached.clone(),
                "host",
                Default::default(),
            ),
            nvf_assets::anim_play::Playing::new("host", &host_clip, |_| None),
            Transform::IDENTITY,
        ))
        .id();
    app.init_resource::<nvf_play::phys::ObjectPoses>();
    app.world_mut()
        .run_system_cached(crate::object_anims::publish_poses)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<nvf_play::phys::ObjectPoses>()
            .playing
            .get(&REFERENCE.0),
        Some(&("HostSequence".to_string(), 0.))
    );
    assert_eq!(
        app.world_mut()
            .query::<&crate::object_anims::GroupRig>()
            .iter(app.world())
            .count(),
        1
    );
    assert!(app
        .world()
        .get::<crate::object_anims::GroupRig>(host_group)
        .is_some());
    assert!(app
        .world()
        .get::<nvf_assets::material_anim::RigMaterials>(root)
        .is_some());
    assert_eq!(meshes(&mut app).len(), 2);
    for e in meshes(&mut app) {
        assert_eq!(
            app.world().get::<RenderLayers>(e).unwrap(),
            &RenderLayers::layer(3)
        );
    }
    assert_eq!(
        app.world()
            .get::<MeshMaterial3d<NifMaterial>>(neighbor)
            .unwrap()
            .0,
        cached.shapes[0].material
    );
    assert!(
        cached.rig.is_none(),
        "the authored rig never mutates the shared cached static variant"
    );
}

#[test]
fn invalid_model_specs_do_not_spawn_or_change_a_valid_neighbor() {
    let (mut app, _files, _, _) = fixture(false);
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    let before: HashSet<_> = members(&mut app).into_iter().collect();
    for spec in [
        "Socket|../owned/unique.nif",
        "Socket|*broken",
        "Socket|C:/owned/unique.nif",
        "Socket|owned/unique.txt",
    ] {
        query(
            &mut app,
            &format!("AttachModel OwnedReference 3 \"{spec}\""),
        );
        app.update();
        assert_eq!(
            members(&mut app).into_iter().collect::<HashSet<_>>(),
            before
        );
    }
    for mode in [-1, 4] {
        query(
            &mut app,
            &format!("AttachModel OwnedReference {mode} \"Socket|owned/duplicate.nif\""),
        );
        app.update();
        assert_eq!(
            members(&mut app).into_iter().collect::<HashSet<_>>(),
            before
        );
    }
}

#[test]
fn default_player_attachment_uses_third_rig_without_first_person_fallback() {
    let (mut app, _files, _, _) = fixture(false);
    fn player_rig(world: &mut World, label: &str) -> (Entity, ActorRig) {
        let root = world
            .spawn((Transform::IDENTITY, Visibility::Inherited))
            .id();
        let bone = world.spawn((Transform::IDENTITY, ChildOf(root))).id();
        let skel = nvf_assets::actor::PreparedSkeleton {
            names: vec![Some(label.into()), Some("SharedBone".into())],
            parents: vec![None, Some(0)],
            locals: vec![Transform::IDENTITY; 2],
            ..Default::default()
        };
        (
            root,
            ActorRig {
                reference: FormId::PLAYER_REF.0,
                base: FormId::PLAYER_BASE.0,
                body: FormId::PLAYER_BASE.0,
                skeleton: "authored-player".into(),
                skel: Arc::new(skel),
                nodes: vec![root, bone],
                base_scale: 1.,
            },
        )
    }
    let third = player_rig(app.world_mut(), "ThirdRoot");
    let first = player_rig(app.world_mut(), "FirstRoot");
    let third_bone = third.1.nodes[1];
    let first_bone = first.1.nodes[1];
    app.insert_resource(crate::player_body::PlayerBody::synthetic_script_models(
        third,
        Some(first),
    ));
    query(&mut app, "PlayerREF.GetChildBlocks \"\" 1");
    query(&mut app, "PlayerREF.GetChildBlocks \"\" 2");
    app.update();
    assert_eq!(
        attach(&mut app, "PlayerREF", 3, "SharedBone", "unique.nif"),
        XVal::Num(1.)
    );
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    assert_eq!(app.world().get::<Children>(third_bone).unwrap().len(), 1);
    assert!(app.world().get::<Children>(first_bone).is_none());
    assert_eq!(
        query(&mut app, "PlayerREF.GetNifScale \"UniqueShape_Owned\" 1"),
        XVal::Num(1.)
    );
    assert_eq!(
        query(&mut app, "PlayerREF.GetNifScale \"UniqueShape_Owned\" 2"),
        XVal::Num(0.)
    );
    let e = meshes(&mut app)[0];
    assert_eq!(
        app.world().get::<RenderLayers>(e).unwrap(),
        &RenderLayers::layer(crate::sky::BODY_LAYER)
    );
}

#[test]
fn prebirth_transforms_use_the_real_producer_and_expire_after_exact_detach() {
    for cold in [false, true] {
        let (mut app, _files, root, socket) = fixture(cold);
        if cold {
            app.world_mut().despawn(socket);
        }
        query(
            &mut app,
            "OwnedReference.SetNifScale \"UniqueShape_Owned\" 2",
        );
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
        query(
            &mut app,
            "OwnedReference.SetNifScale \"UniqueShape_Owned\" 5",
        );
        if cold {
            app.update();
            app.update();
            assert!(meshes(&mut app).is_empty());
            app.world_mut()
                .spawn((Name::new("Socket"), Transform::IDENTITY, ChildOf(root)));
        }
        app.update();
        let old = meshes(&mut app)[0];
        assert_eq!(
            app.world().get::<Transform>(old).unwrap().scale,
            Vec3::splat(5.)
        );
        assert_eq!(scene(&mut app, "UniqueShape_Owned").scale, 5.);
        // This write captured the old concrete node. Removing and adding the same
        // specification in one producer pass must not transfer it to the new one.
        query(
            &mut app,
            "OwnedReference.SetNifScale \"UniqueShape_Owned\" 9",
        );
        attach(&mut app, "OwnedReference", 2, "Socket", "unique.nif");
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
        app.update();
        let new = meshes(&mut app)[0];
        assert_ne!(new, old);
        assert!(app.world().get::<Transform>(old).is_none());
        assert_eq!(app.world().get::<Transform>(new).unwrap().scale, Vec3::ONE);
        assert_eq!(scene(&mut app, "UniqueShape_Owned").scale, 1.);
        app.update();
        assert_eq!(app.world().get::<Transform>(new).unwrap().scale, Vec3::ONE);
    }
}

#[test]
fn ancestry_bound_refuses_before_spawn_and_accepts_its_last_supported_parent() {
    for depth in [
        nif_attachments::ANCESTOR_LIMIT - 1,
        nif_attachments::ANCESTOR_LIMIT,
    ] {
        let (mut app, _files, root, _) = fixture(false);
        let mut parent = root;
        for _ in 0..depth {
            parent = app
                .world_mut()
                .spawn((Transform::IDENTITY, ChildOf(parent)))
                .id();
        }
        app.world_mut()
            .entity_mut(parent)
            .insert(Name::new("BoundedParent"));
        app.update();
        assert_eq!(
            attach(&mut app, "OwnedReference", 3, "BoundedParent", "unique.nif"),
            XVal::Num(1.)
        );
        app.update();
        assert_eq!(
            meshes(&mut app).len(),
            usize::from(depth < nif_attachments::ANCESTOR_LIMIT)
        );
        if depth < nif_attachments::ANCESTOR_LIMIT {
            assert_eq!(app.world().get::<Children>(parent).unwrap().len(), 1);
        } else {
            assert!(app.world().get::<Children>(parent).is_none());
        }
    }
}

#[test]
fn parent_rename_and_late_insert_keep_the_actual_parent_recipient() {
    let (mut app, _files, _, socket) = fixture(false);
    assert_eq!(
        query(
            &mut app,
            "OwnedReference.RenameNifBlock \"Socket\" \"RenamedSocket\""
        ),
        XVal::Num(1.)
    );
    assert_eq!(
        attach(&mut app, "OwnedReference", 3, "RenamedSocket", "unique.nif"),
        XVal::Num(1.)
    );
    app.update();
    assert_eq!(
        app.world().get::<Name>(socket).unwrap().as_str(),
        "RenamedSocket"
    );
    assert_eq!(app.world().get::<Children>(socket).unwrap().len(), 1);
    assert_eq!(meshes(&mut app).len(), 1);
    attach(&mut app, "OwnedReference", 2, "RenamedSocket", "unique.nif");
    app.update();
    assert!(meshes(&mut app).is_empty());
    assert_eq!(
        query(
            &mut app,
            "InsertNode OwnedReference 3 \"RenamedSocket|LateSocket\""
        ),
        XVal::Num(1.)
    );
    assert_eq!(
        attach(&mut app, "OwnedReference", 3, "LateSocket", "unique.nif"),
        XVal::Num(1.)
    );
    query(
        &mut app,
        "OwnedReference.SetNifScale \"UniqueShape_Owned\" 4",
    );
    app.update();
    assert!(
        meshes(&mut app).is_empty(),
        "the actual parent Commands have not reached the producer yet"
    );
    let parent = app
        .world()
        .get::<Children>(socket)
        .unwrap()
        .iter()
        .find(|e| {
            app.world()
                .get::<Name>(*e)
                .is_some_and(|n| n.as_str() == "LateSocket")
        })
        .unwrap()
        .to_bits();
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    let owned_root = members(&mut app)
        .into_iter()
        .find(|e| app.world().get::<ChildOf>(*e).unwrap().parent().to_bits() == parent)
        .unwrap();
    assert_eq!(
        app.world().get::<Name>(owned_root).unwrap().as_str(),
        "OwnedNifRoot_Owned"
    );
    assert_eq!(scene(&mut app, "UniqueShape_Owned").scale, 4.);
}

#[test]
fn nested_owned_instances_retire_together_without_a_transient_orphan() {
    let (mut app, _files, _, socket) = fixture(false);
    attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif");
    app.update();
    attach(&mut app, "OwnedReference", 3, "BranchA_Owned", "unique.nif");
    app.update();
    assert_eq!(meshes(&mut app).len(), 2);
    let old = members(&mut app);
    assert_eq!(old.len(), 6);
    let outer = old
        .iter()
        .copied()
        .find(|e| app.world().get::<ChildOf>(*e).unwrap().parent() == socket)
        .unwrap();
    let foreign = app
        .world_mut()
        .spawn((
            Name::new("ForeignNeighbor"),
            Transform::IDENTITY,
            ChildOf(outer),
        ))
        .id();
    let inner = old
        .iter()
        .copied()
        .find(|e| {
            app.world()
                .get::<Name>(*e)
                .is_some_and(|n| n.as_str() == "OwnedNifRoot_Owned")
                && *e != outer
        })
        .unwrap();
    let nested_foreign = app
        .world_mut()
        .spawn((
            Name::new("NestedForeign"),
            Transform::IDENTITY,
            ChildOf(inner),
        ))
        .id();
    attach(&mut app, "OwnedReference", 2, "Socket", "unique.nif");
    app.update();
    assert!(members(&mut app).is_empty());
    assert!(old
        .iter()
        .all(|e| app.world().get::<Transform>(*e).is_none()));
    for e in [foreign, nested_foreign] {
        assert!(app.world().get::<Transform>(e).is_some());
        assert!(app.world().get::<ChildOf>(e).is_none());
    }
    app.update();
    assert!(members(&mut app).is_empty());
}

#[test]
fn legacy_edit_drains_retire_the_side_queue_without_disabling_fresh_production() {
    for targeted in [false, true] {
        let (mut app, _files, _, _) = fixture(false);
        assert_eq!(attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"), XVal::Num(1.));
        {
            let mut game = app.world_mut().resource_mut::<TestGame>();
            let scene = &mut game.0.scene;
            let count = if targeted { scene.take_targeted_edits().len() } else { scene.take_edits().len() };
            assert_eq!(count, 1);
            assert!(scene.take_attachment_changes().is_empty(), "the legacy consumer already owns this request");
        }
        app.update(); assert!(meshes(&mut app).is_empty());
        attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"); app.update();
        assert_eq!(meshes(&mut app).len(), 1);
    }
}


#[test]
fn unavailable_assets_wait_without_losing_the_captured_parent_lifetime() {
    for transition in ["same", "replacement", "reincarnation"] {
        let (mut app, _files, root, socket) = fixture(false);
        let cache = app.world_mut().remove_resource::<nvf_assets::ModelCache>().unwrap();
        let data = app.world_mut().remove_resource::<GameData>().unwrap();
        assert_eq!(attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"), XVal::Num(1.));
        app.update();
        assert!(members(&mut app).is_empty());
        if transition == "replacement" {
            app.world_mut().entity_mut(root).remove::<nvf_play::RefVisual>();
            host(app.world_mut(), REFERENCE, "Socket");
        } else if transition == "reincarnation" {
            let transform = *app.world().get::<Transform>(socket).unwrap();
            app.world_mut().entity_mut(socket).remove::<Transform>().insert(transform);
        }
        app.update();
        assert!(members(&mut app).is_empty());
        app.insert_resource(cache).insert_resource(data);
        app.update();
        if transition == "same" {
            assert_eq!(meshes(&mut app).len(), 1, "asset arrival must retry the captured parent");
            assert_eq!(scene(&mut app, "UniqueShape_Owned").info.unwrap().identity.unwrap().model, root.to_bits());
        } else {
            assert!(members(&mut app).is_empty(), "asset arrival cannot revive an expired recipient");
            assert_eq!(attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"), XVal::Num(1.));
            app.update();
            assert_eq!(meshes(&mut app).len(), 1, "an explicit fresh request may use the current recipient");
        }
    }
}

#[test]
fn unavailable_assets_do_not_skip_detach_lifetime_session_or_runtime_cleanup() {
    for reason in ["detach", "lifetime", "session", "runtime"] {
        let (mut app, _files, root, socket) = fixture(false);
        assert_eq!(attach(&mut app, "OwnedReference", 3, "Socket", "unique.nif"), XVal::Num(1.));
        app.update();
        assert_eq!(meshes(&mut app).len(), 1);
        let old = members(&mut app);
        let foreign = app.world_mut().spawn((Name::new("Foreign child"), Transform::IDENTITY, ChildOf(old[0]))).id();
        let cache = app.world_mut().remove_resource::<nvf_assets::ModelCache>().unwrap();
        match reason {
            "detach" => { assert_eq!(attach(&mut app, "OwnedReference", 2, "Socket", "unique.nif"), XVal::Num(0.)); }
            "lifetime" => {
                let transform = *app.world().get::<Transform>(socket).unwrap();
                app.world_mut().entity_mut(socket).remove::<Transform>().insert(transform);
            }
            "session" => {
                let game = &app.world().resource::<TestGame>().0;
                let next = Game::new(game.data, fnv_sim::World::new(game.world.forms, game.world.placed, Default::default()), Default::default());
                app.insert_resource(TestGame(next));
            }
            "runtime" => {
                // The real registered entry sees no ScriptState here, as after its removal.
                assert!(!app.world().contains_resource::<ScriptState>());
                app.world_mut().run_system_once(nif_attachments::produce).unwrap();
            }
            _ => unreachable!(),
        }
        if reason != "runtime" { app.update(); }
        assert!(members(&mut app).is_empty(), "cleanup must run without a model cache: {reason}");
        assert!(old.iter().all(|e| app.world().get::<Transform>(*e).is_none()));
        assert!(app.world().get::<Transform>(foreign).is_some());
        assert!(app.world().get::<ChildOf>(foreign).is_none());
        assert!(app.world().get::<Transform>(root).is_some());
        assert!(app.world().get::<Transform>(socket).is_some());
        app.insert_resource(cache);
        if reason == "runtime" {
            app.world_mut().run_system_once(nif_attachments::produce).unwrap();
        } else {
            app.update();
        }
        assert!(members(&mut app).is_empty(), "asset restoration alone cannot recreate a retired instance: {reason}");
    }
}


// This additional source differs only by an authored STAT MODL. Keep the original
// twenty controls and their fixture unchanged; the form call resolves a real record.
fn source_with_form_model() -> Source {
    use common::*;
    let references = [
        record(
            b"REFR",
            0,
            REFERENCE.0,
            &[
                sub(b"EDID", z("OwnedReference")),
                sub(b"NAME", u32s(&[0x301])),
                sub(b"DATA", vec![0; 24]),
            ],
        ),
        record(
            b"REFR",
            0,
            OTHER.0,
            &[
                sub(b"EDID", z("OwnedOtherReference")),
                sub(b"NAME", u32s(&[0x301])),
                sub(b"DATA", vec![0; 24]),
            ],
        ),
    ]
    .concat();
    let cell = [
        record(
            b"CELL",
            0,
            0x900,
            &[sub(b"EDID", z("OwnedCell")), sub(b"DATA", vec![1])],
        ),
        group(
            0x900u32.to_le_bytes(),
            6,
            &group(0x900u32.to_le_bytes(), 9, &references),
        ),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(
            b"STAT",
            &record(
                b"STAT",
                0,
                0x301,
                &[
                    sub(b"EDID", z("OwnedBase")),
                    sub(b"MODL", z("owned/duplicate.nif")),
                ],
            ),
        ),
        top(
            b"CELL",
            &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)),
        ),
    ]
    .concat();
    let plugins = [fnv_esm::Plugin::parse(&bytes, "OwnedAttachment.esm".into()).unwrap()];
    let lo = fnv_esm::LoadOrder::new(&plugins).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
    let catalogue = root.join("script/functions.json");
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let raw = serde_json::from_slice(&std::fs::read(&catalogue).unwrap()).unwrap();
    let data = fnv_game::GameData::build(
        &lo,
        &schema,
        FunctionTable::load(&catalogue).unwrap(),
        &raw,
        &refs,
    );
    Source { data, forms, refs }
}


fn form_model_fixture() -> (App, Files, Entity, Entity) {
    let (mut app, files, root, socket) = fixture(false);
    let source = Box::leak(Box::new(source_with_form_model()));
    app.insert_resource(TestGame(Game::new(
        &source.data,
        fnv_sim::World::new(&source.forms, &source.refs, Default::default()),
        GameConfig { strict: true, ..default() },
    )));
    query(&mut app, "OwnedReference.GetChildBlocks \"\"");
    query(&mut app, "OwnedOtherReference.GetChildBlocks \"\"");
    app.update();
    assert_eq!(prepare(&mut app, "duplicate.nif").shapes.len(), 2);
    assert_eq!(scene(&mut app, "Socket").name, "Socket");
    (app, files, root, socket)
}

#[test]
fn attach_form_model_keeps_its_legacy_edit_without_a_reference_side_request() {
    let (mut app, _files, _, _) = form_model_fixture();
    assert_eq!(
        query(&mut app, "AttachFormModel OwnedReference OwnedBase 3 \"Socket\""),
        XVal::Num(1.0),
    );
    // Inspect immediately: take_edits/take_targeted_edits would clear the side
    // queue and hide the defect. No producer/update or legacy drain precedes this.
    {
        let mut game = app.world_mut().resource_mut::<TestGame>();
        assert!(game.0.scene.take_attachment_changes().is_empty());
        assert!(matches!(
            game.0.scene.take_edits().as_slice(),
            [SceneEdit::Attach { r, target, model }]
                if *r == REFERENCE && target == "Socket" && model == "owned/duplicate.nif"
        ));
    }
    assert_eq!(
        query(&mut app, "HasRuntimeNode OwnedReference \"Socket|owned/duplicate.nif\" 2"),
        XVal::Num(1.0),
    );
}

#[test]
fn attach_form_model_does_not_enroll_the_reference_producer() {
    let (mut app, _files, root, socket) = form_model_fixture();

    // A real positive proves the same app, recipient and model can produce meshes.
    attach(&mut app, "OwnedOtherReference", 3, "Socket", "duplicate.nif");
    app.update();
    assert_eq!(meshes(&mut app).len(), 2);
    attach(&mut app, "OwnedOtherReference", 2, "Socket", "duplicate.nif");
    app.update();
    assert!(meshes(&mut app).is_empty());

    for remove_mode in [2, 0] {
        assert_eq!(
            query(&mut app, "AttachFormModel OwnedReference OwnedBase 3 \"Socket\""),
            XVal::Num(1.0),
        );
        assert_eq!(
            query(&mut app, "HasRuntimeNode OwnedReference \"Socket|owned/duplicate.nif\" 2"),
            XVal::Num(1.0),
        );
        app.update();
        assert_eq!(
            query(&mut app, &format!("AttachFormModel OwnedReference OwnedBase {remove_mode} \"Socket\"")),
            XVal::Num(1.0),
        );
        assert_eq!(
            query(&mut app, "HasRuntimeNode OwnedReference \"Socket|owned/duplicate.nif\" 2"),
            XVal::Num(0.0),
        );
        app.update();
        // First regression boundary: a successful form removal must not leave an
        // instance leaked through the unrelated per-reference side queue.
        assert_eq!(meshes(&mut app).len(), 0, "AttachFormModel leaked physical geometry");
        assert!(members(&mut app).is_empty());
    }
    assert!(app.world().get_entity(root).is_ok());
    assert_eq!(app.world().get::<ChildOf>(socket).unwrap().parent(), root);
    app.world_mut().entity_mut(root).despawn();
    app.update();
    let (replacement, _) = host(app.world_mut(), REFERENCE, "Socket");
    app.update();
    assert!(meshes(&mut app).is_empty(), "form intent must not replay into a new model");
    assert!(members(&mut app).is_empty());
    attach(&mut app, "OwnedReference", 3, "Socket", "duplicate.nif");
    app.update();
    assert_eq!(meshes(&mut app).len(), 2);
    attach(&mut app, "OwnedReference", 2, "Socket", "duplicate.nif");
    app.update();
    assert!(members(&mut app).is_empty());
    assert!(app.world().get_entity(replacement).is_ok());
}

#[allow(clippy::too_many_arguments)]
fn produce_after_queued_foreign_despawn(
    In(foreign): In<Entity>,
    game: ResMut<TestGame>,
    state: ResMut<State>,
    mut commands: Commands,
    models: Option<ModelCtx>,
    rigs: Rigs,
    visuals: Visuals,
    nodes: NodeQuery,
    parents: Query<&ChildOf>,
    layers: Query<&RenderLayers>,
    owned: Owned,
) {
    // The child still exists while the real producer reads Children. Its despawn
    // runs before the producer's queued foreign-child detach at the same barrier.
    commands.entity(foreign).despawn();
    produce(game, state, commands, models, rigs, visuals, nodes, parents, layers, owned);
}

#[test]
fn cleanup_tolerates_a_foreign_child_despawned_before_its_detach_applies() {
    let (mut app, _files, root, socket) = fixture(false);
    attach(&mut app, "OwnedReference", 3, "Socket", "duplicate.nif");
    app.update();
    let drawn = meshes(&mut app);
    assert_eq!(drawn.len(), 2);
    let retired_members = members(&mut app);
    let doomed = app.world_mut().spawn((Transform::default(), ChildOf(drawn[0]))).id();
    let survivor = app.world_mut().spawn((Transform::default(), ChildOf(drawn[0]))).id();
    assert_eq!(app.world().get::<ChildOf>(doomed).unwrap().parent(), drawn[0]);
    assert_eq!(app.world().get::<ChildOf>(survivor).unwrap().parent(), drawn[0]);
    attach(&mut app, "OwnedReference", 2, "Socket", "duplicate.nif");
    app.world_mut()
        .run_system_once_with(produce_after_queued_foreign_despawn, doomed)
        .unwrap();
    assert!(app.world().get_entity(doomed).is_err());
    assert!(app.world().get_entity(survivor).is_ok());
    assert!(app.world().get::<ChildOf>(survivor).is_none());
    assert!(retired_members.iter().all(|e| app.world().get_entity(*e).is_err()));
    assert_eq!(app.world().get::<ChildOf>(socket).unwrap().parent(), root);
    app.update();
    assert!(members(&mut app).is_empty());
    assert!(app.world().get_entity(survivor).is_ok());
    assert!(app.world().get_entity(root).is_ok());
}
