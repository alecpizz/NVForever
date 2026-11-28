//! Authored Struct controls for the shared numeric admission path. No game files.
use super::*;

fn strukt<const N: usize>(fields: [(&str, Value); N]) -> Struct {
    Struct { fields: fields.into_iter().map(|(name, value)| (name.into(), value)).collect() }
}

fn replace(s: &mut Struct, name: &str, value: Value) {
    s.fields.push((name.into(), value));
}

fn geometry() -> Struct {
    strukt([
        ("vertices", Value::FloatList { dim: 3, data: vec![-0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0] }),
        ("normals", Value::FloatList { dim: 3, data: [0.0, 0.0, 1.0].repeat(3) }),
        ("uv_sets", Value::List(vec![Value::FloatList { dim: 2, data: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0] }])),
        ("triangles", Value::IntList(vec![0, 1, 2])),
    ])
}

fn bind() -> Struct {
    strukt([
        ("translation", Value::Floats(vec![0.0; 3])),
        ("rotation", Value::Floats(NifTransform::IDENTITY.rotation.to_vec())),
        ("scale", Value::Float(1.0)),
    ])
}

fn skin_data(weights: &[f32]) -> Struct {
    let bones = weights.iter().map(|&weight| Value::Struct(strukt([
        ("skin_transform", Value::Struct(bind())),
        ("vertex_weights", Value::List(vec![Value::Struct(strukt([
            ("index", Value::Int(0)), ("weight", Value::Float(weight)),
        ]))])),
    ]))).collect();
    strukt([("bone_list", Value::List(bones))])
}

fn skin_fixture(data: Struct, bones: usize) -> (Decoded, Struct) {
    let instance = strukt([
        ("bones", Value::RefList((0..bones).map(|i| i as i32 + 2).collect())),
        ("data", Value::Ref(1)),
    ]);
    let mut blocks = vec![Ok(instance), Ok(data)];
    blocks.extend((0..bones).map(|_| Ok(Struct::default())));
    let count = blocks.len();
    (Decoded { blocks, lineage: vec![Vec::new(); count] }, strukt([("skin_instance", Value::Ref(0))]))
}

fn empty_decoded() -> Decoded {
    Decoded { blocks: Vec::new(), lineage: Vec::new() }
}

fn weights(input: &[f32], vertices: usize) -> SkinWeights {
    skin_weights(&skin_data(input), vertices).expect("finite input").expect("influences")
}

#[test]
fn finite_positive_weights_keep_joint_order_and_normalization() {
    let (joints, actual) = weights(&[2.0, 6.0], 1);
    assert_eq!(joints, [[1, 0, 0, 0]]);
    assert_eq!(actual, [[0.75, 0.25, 0.0, 0.0]]);
}

#[test]
fn finite_maximum_weights_normalize_and_generate_finite_joint_bounds() {
    let (dec, shape) = skin_fixture(skin_data(&[f32::MAX, f32::MAX]), 2);
    let AdmittedShape { mut mesh, skin, .. } = admit_shape(&dec, &shape, &geometry()).unwrap().unwrap();
    let skin = skin.unwrap();
    assert_eq!(skin.weights.1[0], [0.5, 0.5, 0.0, 0.0]);
    let binds = skin.insert_into(&mut mesh);
    assert!(binds.iter().all(|m| m.is_finite()));
    assert!(finite_mesh(&mesh).is_ok());
    let mesh = mesh.with_generated_skinned_mesh_bounds().expect("actual Bevy bounds");
    let bounds = mesh.skinned_mesh_bounds().unwrap();
    assert!(!bounds.aabbs.is_empty());
    assert!(bounds.aabbs.iter().all(|b| b.center.is_finite() && b.half_size.is_finite()));
    let all = bounds.aabbs.iter().find(|b| b.half_size == Vec3::new(1.0, 1.5, 0.0)).expect("bone zero keeps full authored triangle bounds");
    assert_eq!(all.center, Vec3::new(1.0, 1.5, 0.0));
}

#[test]
fn top_four_selection_and_equal_weight_stability_are_retained() {
    let (joints, actual) = weights(&[1.0, 2.0, 3.0, 4.0, 5.0], 1);
    assert_eq!(joints[0], [4, 3, 2, 1]);
    assert_eq!(actual[0], [5.0_f64 / 14.0, 4.0 / 14.0, 3.0 / 14.0, 2.0 / 14.0].map(|w| w as f32));
    assert_eq!(weights(&[1.0; 5], 1).0[0], [0, 1, 2, 3]);
}

#[test]
fn finite_signed_weights_remain_supported() {
    assert_eq!(weights(&[2.0, -1.0], 1).1[0], [2.0, -1.0, 0.0, 0.0]);
    assert_eq!(weights(&[0.5, 0.5, -0.5], 1).1[0], [1.0, 1.0, -1.0, 0.0]);
}

#[test]
fn nonpositive_and_unweighted_vertices_keep_positive_zero_fallback() {
    for input in [vec![0.0, -0.0], vec![1.0, -1.0], vec![-1.0, -2.0]] {
        let (_, actual) = weights(&input, 2);
        for vertex in actual {
            assert_eq!(vertex.map(f32::to_bits), [1.0_f32.to_bits(), 0, 0, 0]);
        }
    }
}

#[test]
fn nonfinite_influences_are_rejected_before_top_four_selection() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let input = [1.0, 2.0, 3.0, 4.0, bad];
        assert_eq!(skin_weights(&skin_data(&input), 1).err(), Some(NumericError { stream: NumericStream::Weight, element: 0 }));
    }
}

#[test]
fn finite_signed_cancellation_cannot_upload_infinite_normalized_weights() {
    let input = [f32::MAX, f32::MIN_POSITIVE, -f32::MAX];
    assert_eq!(skin_weights(&skin_data(&input), 1).err(), Some(NumericError { stream: NumericStream::NormalizedWeight, element: 0 }));
    // A representable signed residual remains supported, without clamping signs.
    assert_eq!(weights(&[16.0, 1.0, -16.0], 1).1[0], [16.0, 1.0, -16.0, 0.0]);
}

#[test]
fn out_of_range_weight_indices_keep_existing_ignored_semantics() {
    let entries = [-1, 99, 0].into_iter().map(|index| Value::Struct(strukt([
        ("index", Value::Int(index)),
        ("weight", Value::Float(if index == 0 { 1.0 } else { f32::NAN })),
    ]))).collect();
    let data = strukt([("bone_list", Value::List(vec![Value::Struct(strukt([
        ("vertex_weights", Value::List(entries)),
    ]))]))]);
    assert_eq!(skin_weights(&data, 1).unwrap().unwrap().1[0], [1.0, 0.0, 0.0, 0.0]);
}

#[test]
fn nonfinite_positions_reject_the_whole_shape() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut data = geometry();
        let mut positions = data.floats("vertices").unwrap().to_vec();
        positions[4] = bad;
        replace(&mut data, "vertices", Value::FloatList { dim: 3, data: positions });
        assert_eq!(admit_shape(&empty_decoded(), &Struct::default(), &data).err(), Some(NumericError { stream: NumericStream::Position, element: 1 }));
    }
}

#[test]
fn cardinality_accepted_normals_and_uvs_must_be_finite() {
    let mut normals = geometry();
    replace(&mut normals, "normals", Value::FloatList { dim: 3, data: vec![f32::NAN; 9] });
    assert_eq!(build_mesh(&normals).err(), Some(NumericError { stream: NumericStream::Normal, element: 0 }));
    let mut uv = geometry();
    replace(&mut uv, "uv_sets", Value::List(vec![Value::FloatList { dim: 2, data: vec![f32::INFINITY; 6] }]));
    assert_eq!(build_mesh(&uv).err(), Some(NumericError { stream: NumericStream::Uv, element: 0 }));
}

#[test]
fn ignored_optional_streams_and_triangle_filter_remain_unchanged() {
    let mut data = geometry();
    replace(&mut data, "normals", Value::FloatList { dim: 3, data: vec![f32::NAN; 3] });
    replace(&mut data, "uv_sets", Value::List(vec![Value::FloatList { dim: 2, data: vec![f32::NAN; 2] }]));
    replace(&mut data, "triangles", Value::IntList(vec![0, 1, 2, 0, 1, 90]));
    let (mesh, reported, _) = build_mesh(&data).unwrap().unwrap();
    assert_eq!(reported, 2, "existing report counts original triangles");
    assert_eq!(mesh.indices().unwrap().iter().collect::<Vec<_>>(), [0, 1, 2]);
    assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
    let VertexAttributeValues::Float32x3(normals) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap() else { panic!("normal stream") };
    assert_eq!(normals, &vec![[0.0, 0.0, 1.0]; 3]);
    assert!(finite_mesh(&mesh).is_ok());
}

#[test]
fn admitted_positions_keep_signed_zero_and_original_fixed_geometry() {
    let data = geometry();
    let (mesh, tris, _) = build_mesh(&data).unwrap().unwrap();
    assert_eq!(tris, 1);
    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!("positions") };
    assert_eq!(positions.iter().flatten().map(|x| x.to_bits()).collect::<Vec<_>>(), data.floats("vertices").unwrap().iter().map(|x| x.to_bits()).collect::<Vec<_>>());
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&mut self, _: &str, _: bool) -> Option<Handle<Image>> { None }
    fn lacks_alpha(&self, _: &Handle<Image>) -> bool { false }
    fn alpha_is_mask(&self, _: &str) -> bool { false }
    fn alpha_is_opaque(&self, _: &str) -> bool { false }
    fn resolve(&self, raw_path: &str) -> String { raw_path.to_string() }
    fn pbr_set(&mut self, _: &str) -> Option<crate::authored::PbrSet> { None }
    fn authored_on(&self) -> bool { false }
}

#[test]
fn vertex_color_repair_runs_before_final_numeric_admission() {
    let mut data = geometry();
    replace(&mut data, "vertex_colors", Value::FloatList { dim: 4, data: [f32::NAN, -3.0, 2.0, f32::INFINITY].repeat(3) });
    let (mut mesh, _, _) = build_mesh(&data).unwrap().unwrap();
    let _ = shape_material_with(&Props { vertex_alpha: true, ..Default::default() }, &mut mesh, &mut NoTextures, Tangents::Generate);
    assert!(finite_mesh(&mesh).is_ok());
    let VertexAttributeValues::Float32x4(colors) = mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() else { panic!("color stream") };
    assert_eq!(colors, &vec![[1.0, 0.0, 1.0, 1.0]; 3]);
}

#[test]
fn nonfinite_inverse_bind_and_finite_matrix_overflow_are_rejected() {
    for transform in [
        strukt([("translation", Value::Floats(vec![f32::NAN, 0.0, 0.0])), ("rotation", Value::Floats(NifTransform::IDENTITY.rotation.to_vec())), ("scale", Value::Float(1.0))]),
        strukt([("translation", Value::Floats(vec![0.0; 3])), ("rotation", Value::Floats(vec![2.0; 9])), ("scale", Value::Float(f32::MAX))]),
    ] {
        let mut data = skin_data(&[1.0]);
        let Value::List(bones) = &mut data.fields[0].1 else { unreachable!() };
        let Value::Struct(bone) = &mut bones[0] else { unreachable!() };
        replace(bone, "skin_transform", Value::Struct(transform));
        let (dec, shape) = skin_fixture(data, 1);
        assert_eq!(admit_shape(&dec, &shape, &geometry()).err(), Some(NumericError { stream: NumericStream::InverseBind, element: 0 }));
    }
}

#[test]
fn absent_skin_is_distinct_from_numeric_rejection_and_neighbors_survive() {
    let mut rejected = NumericRejections::default();
    let plain = rejected.admit_android(7, &empty_decoded(), &Struct::default(), &geometry()).unwrap();
    assert!(plain.skin.is_none());
    let (dec, bad) = skin_fixture(skin_data(&[f32::NAN]), 1);
    assert!(rejected.admit_android(8, &dec, &bad, &geometry()).is_none(), "malformed skin must not yield Some(unskinned shape)");
    let (dec, good) = skin_fixture(skin_data(&[1.0]), 1);
    assert!(rejected.admit_android(9, &dec, &good, &geometry()).unwrap().skin.is_some());
    assert_eq!(rejected.total, 1);
    assert_eq!(rejected.first, [(8, NumericError { stream: NumericStream::Weight, element: 0 })]);
}

#[cfg(not(target_os = "android"))]
#[test]
fn desktop_admission_keeps_main_behavior_without_dropping_shapes() {
    let mut rejected = NumericRejections::default();
    let mut data = geometry();
    let mut positions = data.floats("vertices").unwrap().to_vec();
    positions[4] = f32::NAN;
    replace(&mut data, "vertices", Value::FloatList { dim: 3, data: positions });
    let admitted = rejected.admit(1, &empty_decoded(), &Struct::default(), &data);
    assert!(admitted.is_some(), "desktop admission does not reject nonfinite shapes");
    let admitted = admitted.unwrap();
    assert!(rejected.accept_mesh(1, &admitted.mesh));
    assert_eq!(rejected.total, 0);
    assert!(!rejected.accept_mesh_android(1, &admitted.mesh), "Android path rejects the same nonfinite mesh");
    assert_eq!(rejected.total, 1);
}

#[test]
fn final_stream_check_catches_generated_tangent_and_weight_nonfinites() {
    for (attribute, stream) in [(Mesh::ATTRIBUTE_TANGENT, NumericStream::Tangent), (Mesh::ATTRIBUTE_JOINT_WEIGHT, NumericStream::NormalizedWeight)] {
        let (mut mesh, _, _) = build_mesh(&geometry()).unwrap().unwrap();
        mesh.insert_attribute(attribute, vec![[1.0_f32, 0.0, 0.0, f32::NAN]; 3]);
        assert_eq!(finite_mesh(&mesh), Err(NumericError { stream, element: 0 }));
    }
}

#[test]
fn diagnostics_keep_only_eight_samples_and_saturate_the_total() {
    let error = NumericError { stream: NumericStream::Position, element: 2 };
    let mut rejected = NumericRejections::default();
    for block in 0..1000 { rejected.record(block, error); }
    assert_eq!(rejected.total, 1000);
    assert_eq!(rejected.first, (0..8).map(|b| (b, error)).collect::<Vec<_>>());
    rejected.total = usize::MAX;
    rejected.record(9999, error);
    assert_eq!(rejected.total, usize::MAX);
    assert_eq!(rejected.first.len(), 8);
}
