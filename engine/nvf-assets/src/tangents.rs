//! Tangents for normal maps read from the model files instead of generated
//! (`Mesh::generate_tangents`, mikktspace), for the static models (`crate::model`): the
//! generator was about 80 % of their preparation [observed: `NVF_LOADING_PROBE`, Novac
//! start, 2.6-3.8 s of the read pool's time and 0.33-0.46 s on the main thread under the
//! loading screen]. docs/spec-notes/nif.md §6 "Stored tangents".
//!
//! - A shape-data block whose flags have bit 12 set stores, after its normals, an array of
//!   "Tangents" and one of "Bitangents", a vector per vertex each [documented: nif.xml
//!   NiGeometryData, BSGeometryDataFlags bit 12 "Has Tangents"; `schemas/nif/core.json`].
//!   nif.xml says nothing of their axes or handedness.
//! - The files' "bitangents" run along increasing u and their "tangents" along increasing
//!   v, the other way round from the usual names [observed: nif.md §6, against the
//!   triangles' texture gradients]. Bevy's tangent (xyz, along increasing u) is the stored
//!   "bitangent" made orthogonal to the normal, as the generator's are; its sign w (Bevy's
//!   bitangent is w × cross(normal, tangent), `pbr_functions.wesl`
//!   `calculate_tbn_mikktspace`, and points up the image, towards decreasing v) is the one
//!   that turns that bitangent towards minus the stored "tangent" [observed: nif.md §6,
//!   97.8 % of 13.65 million vertices within 25 degrees of the generator's, sign included].
//! - Where the stored tangent lies along the stored bitangent (the texture squeezed to a
//!   line there), the sign is the one the tiny remainder gives, + when there is none
//!   [engineering choice: such vertices are 0.1 % of those of the shapes that store
//!   tangents, and the generator's own sign there agreed with it at 72 %; generating the
//!   whole shape's for them left a fifth of the vertices to the generator; nif.md §6].
//! - A shape whose arrays are missing, of another length than its vertices, not finite or
//!   degenerate (a zero vector, a bitangent along the normal) at a vertex a triangle uses
//!   gets generated tangents, as before [design]. Vertices no triangle uses are not drawn,
//!   so they take any unit tangent.
//! - Actor parts (`crate::actor`) keep generating theirs; only `crate::model` asks for
//!   [`Tangents::Stored`].
//!
//! Test aid `NVF_STORED_TANGENTS=0`: every shape's tangents are generated, as before.
//! `summary` gives the run's counts (`nvforever` logs it at exit).

use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Where a shape's tangents come from when its normal map needs them.
#[derive(Clone, Copy, Debug)]
pub enum Tangents<'a> {
    /// Generated (mikktspace).
    Generate,
    /// The mesh's own when it has them (a copy of a mesh that had them generated before),
    /// generated otherwise.
    Keep,
    /// Those stored in the shape-data block (module notes) when usable and the test aid
    /// allows them, generated otherwise.
    Stored(&'a StoredArrays),
}

/// The tangent arrays a shape-data block stores, one vector per vertex each, as
/// `fnv_nif::scene::mesh_data` decodes them (`crate::nif_spawn::build_mesh` hands them on);
/// empty when the block stores none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoredArrays {
    /// The file's "Tangents" (along increasing v, module notes).
    pub tangents: Vec<[f32; 3]>,
    /// The file's "Bitangents" (along increasing u).
    pub bitangents: Vec<[f32; 3]>,
}

impl Tangents<'_> {
    /// Give `mesh` its tangents; whether it has them.
    pub(crate) fn add_to(self, mesh: &mut Mesh) -> bool {
        self.add(mesh, enabled())
    }

    /// [`Self::add_to`], stored tangents allowed or not (the test aid, [`enabled`]).
    fn add(self, mesh: &mut Mesh, stored_allowed: bool) -> bool {
        match self {
            Tangents::Keep if mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some() => true,
            Tangents::Stored(data) if stored_allowed => match for_mesh(data, mesh) {
                Ok(s) => {
                    count(&STORED, 1);
                    count(&STORED_VERTICES, s.tangents.len() as u64);
                    count(&OPEN_SIGNS, s.open_signs as u64);
                    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, s.tangents);
                    true
                }
                Err(why) => {
                    count(why.counter(), 1);
                    mesh.generate_tangents().is_ok()
                }
            },
            Tangents::Stored(_) => {
                count(&GENERATED_AID, 1);
                mesh.generate_tangents().is_ok()
            }
            _ => mesh.generate_tangents().is_ok(),
        }
    }
}

/// Why a shape's stored tangents are not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unusable {
    /// The block stores none (or the mesh has no normals).
    Absent,
    /// An array's length is not one vector per vertex.
    Count,
    /// A used vertex has a value that is not finite.
    NotFinite,
    /// A used vertex has a zero vector or a bitangent along its normal.
    Degenerate,
}

impl Unusable {
    fn counter(self) -> &'static AtomicU64 {
        match self {
            Unusable::Absent => &ABSENT,
            Unusable::Count => &COUNT,
            Unusable::NotFinite => &NOT_FINITE,
            Unusable::Degenerate => &DEGENERATE,
        }
    }
}

/// Whether stored tangents are used: unless the test aid `NVF_STORED_TANGENTS=0`.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = aid_allows(std::env::var("NVF_STORED_TANGENTS").ok().as_deref());
        if on {
            info!("models: normal-map tangents of static models read from their files where stored and usable (NVF_STORED_TANGENTS=0 generates them all)");
        } else {
            info!("test aid NVF_STORED_TANGENTS=0: every model's normal-map tangents generated (mikktspace), none read from the files");
        }
        on
    })
}

/// The test aid's value read: anything but `0` leaves stored tangents on.
fn aid_allows(value: Option<&str>) -> bool {
    value.map(str::trim) != Some("0")
}

/// A shape's Bevy tangents read from its file.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    /// Per vertex: unit xyz, w = ±1.
    pub tangents: Vec<[f32; 4]>,
    /// Used vertices whose sign the arrays leave open (module notes).
    pub open_signs: usize,
}

/// Bevy tangents for `mesh` from the arrays stored in its shape-data block
/// ([`from_stored`] over the mesh's normals and the vertices its triangles use).
pub fn for_mesh(stored: &StoredArrays, mesh: &Mesh) -> Result<Stored, Unusable> {
    if stored.tangents.is_empty() || stored.bitangents.is_empty() {
        return Err(Unusable::Absent);
    }
    let (tangents, bitangents) = (stored.tangents.as_flattened(), stored.bitangents.as_flattened());
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { return Err(Unusable::Absent) };
    let mut used = vec![false; normals.len()];
    if let Some(indices) = mesh.indices() {
        for i in indices.iter() {
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
        }
    }
    from_stored(normals, tangents, bitangents, &used)
}

/// Bevy tangents (unit xyz, w = ±1) from per-vertex `normals` and a shape's stored
/// `tangents` and `bitangents` (three floats a vertex each), the vertices of `used` checked
/// (module notes): xyz is the stored bitangent with its part along the normal taken out,
/// w the sign that turns w × cross(normal, xyz) towards minus the stored tangent.
pub fn from_stored(normals: &[[f32; 3]], tangents: &[f32], bitangents: &[f32], used: &[bool]) -> Result<Stored, Unusable> {
    let n = normals.len();
    if tangents.len() != 3 * n || bitangents.len() != 3 * n || used.len() != n {
        return Err(Unusable::Count);
    }
    let mut out = Stored { tangents: Vec::with_capacity(n), open_signs: 0 };
    for (i, normal) in normals.iter().enumerate() {
        let at = |a: &[f32]| Vec3::new(a[3 * i], a[3 * i + 1], a[3 * i + 2]);
        match vertex(Vec3::from_array(*normal), at(tangents), at(bitangents)) {
            Ok((t, open)) => {
                out.tangents.push(t);
                out.open_signs += usize::from(open && used[i]);
            }
            Err(_) if !used[i] => out.tangents.push([1.0, 0.0, 0.0, 1.0]),
            Err(why) => return Err(why),
        }
    }
    Ok(out)
}

/// Smallest length of the bitangent once its part along the normal is taken out, and of
/// the tangent's part across the normal and that bitangent, both as a share of unit
/// vectors [engineering choice: about 0.06 degrees from the normal (or from the
/// bitangent); closer, the direction or the sign is noise].
const MIN_PART: f32 = 1e-3;

/// One vertex's Bevy tangent ([`from_stored`]), and whether its sign is left open (the
/// stored tangent along the bitangent, module notes).
fn vertex(normal: Vec3, tangent: Vec3, bitangent: Vec3) -> Result<([f32; 4], bool), Unusable> {
    if !(normal.is_finite() && tangent.is_finite() && bitangent.is_finite()) {
        return Err(Unusable::NotFinite);
    }
    let (Some(n), Some(t), Some(b)) = (normal.try_normalize(), tangent.try_normalize(), bitangent.try_normalize()) else { return Err(Unusable::Degenerate) };
    let across = b - n * n.dot(b);
    if across.length() < MIN_PART {
        return Err(Unusable::Degenerate);
    }
    let x = across.normalize();
    let side = n.cross(x).dot(-t);
    Ok(([x.x, x.y, x.z, if side < 0.0 { -1.0 } else { 1.0 }], side.abs() < MIN_PART))
}

static STORED: AtomicU64 = AtomicU64::new(0);
static STORED_VERTICES: AtomicU64 = AtomicU64::new(0);
static OPEN_SIGNS: AtomicU64 = AtomicU64::new(0);
static ABSENT: AtomicU64 = AtomicU64::new(0);
static COUNT: AtomicU64 = AtomicU64::new(0);
static NOT_FINITE: AtomicU64 = AtomicU64::new(0);
static DEGENERATE: AtomicU64 = AtomicU64::new(0);
static GENERATED_AID: AtomicU64 = AtomicU64::new(0);

fn count(c: &AtomicU64, n: u64) {
    c.fetch_add(n, Ordering::Relaxed);
}

/// The run's static-model shapes whose normal map needed tangents, by where they came
/// from: `models: normal-map tangents of 812 shapes read from their files (95310
/// vertices, 41 signs left open), 3 generated (none stored 1, wrong length 0, not finite 0,
/// degenerate 2, test aid 0)`.
pub fn summary() -> String {
    let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
    let generated = g(&ABSENT) + g(&COUNT) + g(&NOT_FINITE) + g(&DEGENERATE) + g(&GENERATED_AID);
    format!(
        "models: normal-map tangents of {} shapes read from their files ({} vertices, {} signs left open), {generated} generated (none stored {}, wrong length {}, not finite {}, degenerate {}, test aid {})",
        g(&STORED),
        g(&STORED_VERTICES),
        g(&OPEN_SIGNS),
        g(&ABSENT),
        g(&COUNT),
        g(&NOT_FINITE),
        g(&DEGENERATE),
        g(&GENERATED_AID)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    /// A quad in the XY plane facing +Z, its texture's u along +X and v along -Y (the
    /// image's rows down the screen) or, `mirrored`, along +Y.
    fn quad(mirrored: bool) -> Mesh {
        let v = |y: f32| if mirrored { y } else { 1.0 - y };
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, v(0.0)], [1.0, v(0.0)], [1.0, v(1.0)], [0.0, v(1.0)]]);
        m.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        m
    }

    /// The arrays a file stores for a shape (module notes, nif.md §6): "bitangents" along
    /// increasing u, "tangents" along increasing v (`texture_gradients`).
    fn stored_like_the_files(mesh: &Mesh) -> (Vec<f32>, Vec<f32>) {
        let (du, dv) = texture_gradients(mesh);
        (dv.iter().flat_map(|v| v.to_array()).collect(), du.iter().flat_map(|v| v.to_array()).collect())
    }

    /// Per vertex, the sum over its triangles of the position's change along u and v.
    fn texture_gradients(mesh: &Mesh) -> (Vec<Vec3>, Vec<Vec3>) {
        let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        let idx: Vec<usize> = mesh.indices().unwrap().iter().collect();
        let (mut du, mut dv) = (vec![Vec3::ZERO; p.len()], vec![Vec3::ZERO; p.len()]);
        for t in idx.chunks(3) {
            let (p0, p1, p2) = (Vec3::from(p[t[0]]), Vec3::from(p[t[1]]), Vec3::from(p[t[2]]));
            let (w0, w1, w2) = (Vec2::from(uv[t[0]]), Vec2::from(uv[t[1]]), Vec2::from(uv[t[2]]));
            let (e1, e2, s1, s2) = (p1 - p0, p2 - p0, w1 - w0, w2 - w0);
            let r = 1.0 / (s1.x * s2.y - s2.x * s1.y);
            let (gu, gv) = ((e1 * s2.y - e2 * s1.y) * r, (e2 * s1.x - e1 * s2.x) * r);
            for &k in t {
                du[k] += gu;
                dv[k] += gv;
            }
        }
        (du, dv)
    }

    fn generated(mut mesh: Mesh) -> Vec<[f32; 4]> {
        mesh.generate_tangents().unwrap();
        let Some(VertexAttributeValues::Float32x4(t)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        t.clone()
    }

    fn data(tangents: Vec<f32>, bitangents: Vec<f32>) -> StoredArrays {
        StoredArrays { tangents: tangents.as_chunks::<3>().0.to_vec(), bitangents: bitangents.as_chunks::<3>().0.to_vec() }
    }

    fn assert_close(ours: &[[f32; 4]], theirs: &[[f32; 4]]) {
        assert_eq!(ours.len(), theirs.len());
        for (a, b) in ours.iter().zip(theirs) {
            assert!(Vec3::from_slice(a).dot(Vec3::from_slice(b)) > 0.999, "{a:?} against {b:?}");
            assert_eq!(a[3], b[3], "the sign: {a:?} against {b:?}");
        }
    }

    #[test]
    fn stored_arrays_give_the_generators_frame() {
        for mirrored in [false, true] {
            let mesh = quad(mirrored);
            let (t, b) = stored_like_the_files(&mesh);
            let used = vec![true; 4];
            let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
            let ours = from_stored(normals, &t, &b, &used).unwrap();
            assert_eq!(ours.open_signs, 0);
            let ours = ours.tangents;
            assert_close(&ours, &generated(mesh.clone()));
            // The image's rows down the screen (v along -Y): Bevy's bitangent is up it, +Y,
            // the same side as cross(normal, tangent).
            assert_eq!(ours[0][3], if mirrored { -1.0 } else { 1.0 }, "mirrored {mirrored}");
        }
    }

    #[test]
    fn the_bitangent_loses_its_part_along_the_normal() {
        let (t, open) = vertex(Vec3::Z, Vec3::Y, Vec3::new(1.0, 0.0, 0.5)).unwrap();
        assert!((Vec3::from_slice(&t) - Vec3::X).length() < 1e-6, "{t:?}");
        assert_eq!((t[3], open), (-1.0, false));
        assert_eq!(vertex(Vec3::Z, -Vec3::Y, Vec3::X).unwrap().0[3], 1.0);
    }

    #[test]
    fn a_tangent_along_the_bitangent_leaves_the_sign_to_its_remainder() {
        // The tangent a hair off the bitangent towards -Y: the sign of that remainder.
        let (t, open) = vertex(Vec3::Z, Vec3::new(1.0, -1e-5, 0.0), Vec3::X).unwrap();
        assert_eq!((t, open), ([1.0, 0.0, 0.0, 1.0], true));
        let (t, open) = vertex(Vec3::Z, Vec3::new(1.0, 1e-5, 0.0), Vec3::X).unwrap();
        assert_eq!((t, open), ([1.0, 0.0, 0.0, -1.0], true));
        // None at all: +.
        assert_eq!(vertex(Vec3::Z, Vec3::X, Vec3::X).unwrap(), ([1.0, 0.0, 0.0, 1.0], true));
        // Counted at the vertices triangles use only.
        let normals = [[0.0, 0.0, 1.0]; 2];
        let along_u = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        assert_eq!(from_stored(&normals, &along_u, &along_u, &[true, true]).unwrap().open_signs, 2);
        assert_eq!(from_stored(&normals, &along_u, &along_u, &[false, true]).unwrap().open_signs, 1);
    }

    #[test]
    fn unusable_arrays_are_named() {
        let normals = [[0.0, 0.0, 1.0]; 2];
        let used = [true, true];
        let good = [0.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        let along_u = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        assert!(from_stored(&normals, &good, &along_u, &used).is_ok());
        assert_eq!(from_stored(&normals, &good[..3], &along_u, &used), Err(Unusable::Count));
        assert_eq!(from_stored(&normals, &good, &along_u[..3], &used), Err(Unusable::Count));
        let nan = [f32::NAN, 1.0, 0.0, 0.0, 1.0, 0.0];
        assert_eq!(from_stored(&normals, &nan, &along_u, &used), Err(Unusable::NotFinite));
        let zero = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(from_stored(&normals, &zero, &along_u, &used), Err(Unusable::Degenerate));
        // A bitangent along the normal has no direction across it.
        let up = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(from_stored(&normals, &good, &up, &used), Err(Unusable::Degenerate));
        // A vertex no triangle uses does not count.
        let t = from_stored(&normals, &nan, &along_u, &[false, true]).unwrap().tangents;
        assert_eq!(t[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(t[1], [1.0, 0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_shape_without_usable_arrays_gets_generated_tangents() {
        let mesh = quad(false);
        let want = generated(mesh.clone());
        let mut none = mesh.clone();
        assert!(Tangents::Stored(&StoredArrays::default()).add(&mut none, true));
        let Some(VertexAttributeValues::Float32x4(got)) = none.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert_close(got, &want);
        assert_eq!(for_mesh(&StoredArrays::default(), &mesh), Err(Unusable::Absent));
        let (t, b) = stored_like_the_files(&mesh);
        assert_eq!(for_mesh(&data(t[..9].to_vec(), b.clone()), &mesh), Err(Unusable::Count));
        let mut broken = mesh.clone();
        let mut nan = t.clone();
        nan[0] = f32::NAN;
        assert!(Tangents::Stored(&data(nan, b.clone())).add(&mut broken, true));
        let Some(VertexAttributeValues::Float32x4(got)) = broken.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert_close(got, &want);
        let mut no_normals = mesh.clone();
        no_normals.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
        assert_eq!(for_mesh(&data(t, b), &no_normals), Err(Unusable::Absent));
    }

    #[test]
    fn a_shape_with_usable_arrays_takes_them() {
        let mesh = quad(true);
        // Stored tangents a little off the generator's: the result is the stored ones.
        let (mut t, mut b) = stored_like_the_files(&mesh);
        b[1] = 0.2;
        t[0] = 0.1;
        let mut ours = mesh.clone();
        assert!(Tangents::Stored(&data(t.clone(), b.clone())).add(&mut ours, true));
        let Some(VertexAttributeValues::Float32x4(got)) = ours.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        assert_eq!(got, &from_stored(normals, &t, &b, &[true; 4]).unwrap().tangents);
        assert!(Vec3::from_slice(&got[0]).dot(Vec3::from_slice(&b[..3]).normalize()) > 0.9999);
        // The test aid (`NVF_STORED_TANGENTS=0`): the same arrays, generated tangents.
        let mut aid = mesh.clone();
        assert!(Tangents::Stored(&data(t, b)).add(&mut aid, false));
        let Some(VertexAttributeValues::Float32x4(got)) = aid.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert_close(got, &generated(mesh));
    }

    /// `build_mesh` hands on the arrays the shape-data block's decoding (`mesh_data`)
    /// already read, so they are not read from the block a second time.
    #[test]
    fn the_mesh_comes_with_the_blocks_decoded_arrays() {
        use fnv_nif::{Struct, Value};
        let list = |data: Vec<f32>| Value::FloatList { dim: 3, data };
        let block = |stored: bool| {
            let mut fields = vec![
                ("vertices".into(), list(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])),
                ("normals".into(), list(vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0])),
                ("triangles".into(), Value::IntList(vec![0, 1, 2])),
            ];
            if stored {
                fields.push(("tangents".into(), list(vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0])));
                fields.push(("bitangents".into(), list(vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0])));
            }
            Struct { fields }
        };
        let (mesh, _, stored) = crate::nif_spawn::build_mesh(&block(true)).unwrap().unwrap();
        assert_eq!(stored, StoredArrays { tangents: vec![[0.0, 1.0, 0.0]; 3], bitangents: vec![[1.0, 0.0, 0.0]; 3] });
        assert_eq!(for_mesh(&stored, &mesh).unwrap().tangents, vec![[1.0, 0.0, 0.0, -1.0]; 3]);
        let (mesh, _, none) = crate::nif_spawn::build_mesh(&block(false)).unwrap().unwrap();
        assert_eq!(none, StoredArrays::default());
        assert_eq!(for_mesh(&none, &mesh), Err(Unusable::Absent));
    }

    #[test]
    fn keep_and_generate_ignore_the_files() {
        let mesh = quad(false);
        let mut kept = mesh.clone();
        kept.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0, 1.0, 0.0, 1.0]; 4]);
        assert!(Tangents::Keep.add(&mut kept, true));
        let Some(VertexAttributeValues::Float32x4(k)) = kept.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert_eq!(k[0], [0.0, 1.0, 0.0, 1.0]);
        let mut fresh = mesh.clone();
        assert!(Tangents::Generate.add(&mut fresh, true));
        let Some(VertexAttributeValues::Float32x4(g)) = fresh.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        assert_close(g, &generated(mesh));
    }

    #[test]
    fn the_test_aid_turns_them_off_with_0_only() {
        assert!(aid_allows(None));
        assert!(aid_allows(Some("1")));
        assert!(aid_allows(Some("")));
        assert!(!aid_allows(Some("0")));
        assert!(!aid_allows(Some(" 0 ")));
    }
}
