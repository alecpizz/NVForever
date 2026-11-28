//! Scenes made here, without game data, whose rays, points and reach are held against the CPU's.
//!
//! They have to agree: the same surface met (or another in its very place, parallel, with the same light), the same
//! light, the same lamps and shadow rays. Two exemptions, each listed in the output and each bounded by answers the CPU
//! computes on its own:
//! - Where an answer is a rounding's on either side of a limit that the kernel leaves to the structure (an alpha
//!   exactly at the cutoff away from the points, a shadow ray's end), it may be one of those named for it: the CPU's
//!   of scenes a little to either side of the limit, the same triangle, side, place and shadow rays.
//! - From a seam, a ray whose shadow rays start in another surface's plane may see other lamps than the CPU's (they
//!   are the structure's, whose rounding is not the solver's): its light and the lamps it sees are then those of its
//!   triangle alone in the light of a set of the lamps, all of them in sight.
//!
//! A point's counts and moments are the sums of the answers its rays agreed with.

use crate::gpu::Gpu;
use nvf_rtgi::{bounces, classified, directions, meet, prepare, Bounce, Cone, Cull, Directional, Export, Instance, Lamp, LampTable, Level, LightState, Material, Mesh, PreparedScene, Ray, SceneInput, SceneLimits, Texture, Wrap, V3};
use nvf_rtgi_live::{answers_of, meetings_of, probes_of, reach_of, Answer, Config, Points, Tables, Target};

/// Of a ray's light, and of a point's mean.
const TOLERANCE: f32 = 5e-4;
const EMISSION: f32 = 0.25;

/// The one difference that decides nothing: its ray exactly, and why.
struct Known {
    fixture: &'static str,
    /// Flattened, of a point's 512.
    ray: usize,
    /// The CPU's triangle (its hidden back), the GPU's primitive (its front, in sight).
    cpu: u32,
    gpu: u32,
    /// GPU less CPU of the point's hits, backs, lamps and shadow rays.
    delta: [i64; 4],
    /// The kernel's light of it, as first recorded (on the prototype's kernel and on this one, 2026-11-26), and the
    /// lamps it saw.
    light: [f32; 3],
    free: u32,
    why: &'static str,
}

const KNOWN: Known = Known {
    fixture: "from the edge of two walls, near the origin",
    ray: 865,
    cpu: 6,
    gpu: 3,
    delta: [0, -1, 2, 1],
    light: [0.472_521_36, 0.202_482_91, 0.085_289_24],
    free: 1,
    why: "point 1 (one float inside the x wall, on the y wall), direction 353: the CPU meets the y wall's hidden back at their shared edge, the kernel the x wall's front 1e-5 nearer; on the prototype kernel too, its cause not yet found",
};

pub type Parts = Vec<(Mesh, Material)>;
pub type Shows = fn(&[Option<Bounce>]) -> bool;

pub struct Fixture {
    pub name: String,
    pub parts: Parts,
    /// What else a ray's answer may be: that of one of these; of one marked, its place and counts with its lamps in
    /// shadow (no light).
    pub others: Vec<(Parts, LightState, bool)>,
    /// From a seam: a ray whose shadow rays start in another surface's plane may see other lamps.
    pub seam: bool,
    /// Every ray meets the CPU's own triangle: no other in its place.
    pub exact: bool,
    pub textures: Vec<Option<Texture>>,
    pub light: LightState,
    pub from: Vec<V3>,
    pub along: Vec<V3>,
    /// Whether the CPU's answers show what the fixture is for.
    pub shows: Shows,
}

pub fn v(x: f32, y: f32, z: f32) -> V3 {
    V3([x, y, z])
}

pub fn unit(p: V3) -> V3 {
    p.unit().unwrap_or(p)
}

/// A rectangle of half sides `u` and `v`, its front towards `u` x `v`.
pub fn rectangle(centre: V3, u: V3, v: V3, material: Material) -> (Mesh, Material) {
    let positions = [centre - u - v, centre + u - v, centre + u + v, centre - u + v].map(|p| p.0).to_vec();
    (Mesh { positions, uv0: vec![[0.0, 0.0], [3.0, 0.0], [3.0, 2.0], [0.0, 2.0]], indices: vec![0, 1, 2, 0, 2, 3], ..Mesh::default() }, material)
}

pub fn coloured(r: f32, g: f32, b: f32) -> Material {
    Material { base: [r, g, b, 1.0], ..Material::default() }
}

/// A box's six sides, fronts inward, each of its own colour; the one at -z unlit.
pub fn room(centre: V3, half: V3) -> Parts {
    let axis = |k: usize| V3(std::array::from_fn(|i| if i == k % 3 { half[i] } else { 0.0 }));
    let mut out = Vec::new();
    for k in 0..3 {
        for upper in [false, true] {
            let (a, b) = if upper { (axis(k + 2), axis(k + 1)) } else { (axis(k + 1), axis(k + 2)) };
            let m = Material { unlit: k == 2 && !upper, ..coloured(0.25 + 0.25 * k as f32, 0.9 - 0.3 * k as f32, if upper { 0.7 } else { 0.15 }) };
            out.push(rectangle(if upper { centre + axis(k) } else { centre - axis(k) }, a, b, m));
        }
    }
    out
}

pub fn lamp(position: V3, display: V3, range: f32) -> Lamp {
    Lamp { position, display, range, cone: None }
}

/// Ambient, a directional, a lamp that clips a channel, a spot.
pub fn light(at: V3) -> LightState {
    let spot = Lamp { cone: Some(Cone::new(v(0.4, -1.0, 0.2), 0.3, 0.8)), ..lamp(at + v(-50.0, 60.0, 30.0), v(0.2, 0.9, 1.4), 260.0) };
    LightState {
        ambient: v(0.2, 0.3, 0.4),
        directionals: vec![Directional { display: v(0.3, 0.2, 0.1), to_source: unit(v(0.3, 0.9, -0.2)) }],
        lamps: vec![lamp(at + v(40.0, 20.0, -35.0), v(2.5, 0.6, 0.3), 300.0), spot],
    }
}

pub fn one_lamp(position: V3) -> LightState {
    LightState { ambient: V3([0.1; 3]), lamps: vec![lamp(position, V3([0.8; 3]), 300.0)], ..LightState::default() }
}

pub fn lit(b: &[Option<Bounce>]) -> bool {
    b.iter().flatten().any(|b| b.light.max_abs() > 0.01)
}

pub fn any(_: &[Option<Bounce>]) -> bool {
    true
}

impl Fixture {
    pub fn new(name: impl Into<String>, parts: Parts, light: LightState, from: Vec<V3>, along: Vec<V3>, shows: Shows) -> Fixture {
        Fixture { name: name.into(), parts, others: Vec::new(), seam: false, exact: false, textures: Vec::new(), light, from, along, shows }
    }

    fn scene(&self, parts: &Parts) -> SceneInput {
        let mut out = SceneInput { textures: self.textures.clone(), ..SceneInput::default() };
        for (i, (mesh, material)) in parts.iter().enumerate() {
            out.meshes.push(mesh.clone());
            out.materials.push(*material);
            out.instances.push(Instance { mesh: i, material: i, world_from_local: SAME });
        }
        out
    }
}

const SAME: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// Holes by a checkerboard's alpha, coloured.
fn holes() -> Texture {
    let texels = (0..256).map(|i| [200, 40 + (i % 16) as u8 * 10, 90, if (i / 64 + i % 16 / 4) % 2 == 0 { 255 } else { 0 }]).collect();
    Texture { levels: vec![Level { width: 16, height: 16, texels }], srgb: true, wrap: [Wrap::Mirror, Wrap::Repeat] }
}

pub const PLACES: [(&str, V3); 2] = [("near the origin", V3::ZERO), ("near 8000", V3([7936.0, 8000.0, -1792.0]))];

/// A room's lit and unlit sides from inside it, with cutouts, textures and vertex colours.
fn surfaces(out: &mut Vec<Fixture>) {
    let shows: Shows = |b| {
        // Lit ones, and unlit ones.
        let some = |f: &dyn Fn(&Bounce) -> bool| b.iter().flatten().any(f);
        some(&|b| b.shadow_rays > 0 && b.light[0] > 0.2) && some(&|b| b.lamps == 0 && b.light.max_abs() > 0.01)
    };
    let from = vec![v(7.3, -11.1, 3.7), v(-61.7, 43.9, 22.2)];
    out.push(Fixture::new("outgoing light", room(V3::ZERO, v(100.0, 90.0, 120.0)), light(V3::ZERO), from.clone(), directions(512), shows));
    let mut f = Fixture { textures: vec![Some(holes())], ..Fixture::new("cutouts, textures and vertex colours", room(V3::ZERO, v(100.0, 90.0, 120.0)), light(V3::ZERO), from, directions(512), shows) };
    let cut = Material { texture: Some(0), cutoff: 0.5, cull: Cull::None, double_sided: true, uv: [[0.9, 0.1, 0.05], [0.0, 1.1, -0.3]], ..Material::default() };
    let mut fence = rectangle(v(25.0, 0.0, 5.0), v(0.0, 90.0, 0.0), v(0.0, 0.0, 120.0), cut);
    fence.0.colors = vec![[0.9, 1.0, 0.5, 1.0], [1.0; 4], [0.3, 1.0, 1.0, 0.8], [1.0; 4]];
    let mut shown = rectangle(v(-30.0, -80.0, 0.0), v(0.0, 0.0, 60.0), v(60.0, 0.0, 0.0), Material { texture: Some(0), display_colors: true, ..Material::default() });
    shown.0.colors = vec![[0.02, 0.5, 1.0, 1.0], [0.9, 0.03, 0.4, 1.0], [1.0; 4], [0.2; 4]];
    f.parts.extend([fence, shown]);
    out.push(f);
    // A sum of 1.4e-13 to 3e-13, far below 1e-12, under an albedo of about 8.8e23, near the most packing admits (a display
    // colour of 1e6, a factor of 4e9): 5e-5 to 2.5e-4, where a floor of 1e-12 under the power would bring 5.5e-3.
    let mut floor = rectangle(v(7.0, 0.0, 3.0), v(0.0, 0.0, 50.0), v(50.0, 0.0, 0.0), Material { display_colors: true, base: [4e9, 4e9, 4e9, 1.0], ..Material::default() });
    floor.0.colors = vec![[1e6, 1e6, 1e6, 1.0]; 4];
    let faint = LightState { lamps: vec![lamp(v(0.0, 10.0, 0.0), V3([3e-13; 3]), 1000.0)], ..LightState::default() };
    let along = directions(64).into_iter().filter(|d| d[1] < -0.8).collect();
    let small: Shows = |b| b.iter().all(|b| b.is_some_and(|b| b.light.max_abs() > 1e-5 && b.light.max_abs() < 1e-3));
    out.push(Fixture::new("a faint lamp on a surface of a large albedo", vec![floor], faint, vec![v(3.0, 20.0, 4.0)], along, small));
}

/// From where two walls meet, a float beside, of a room's corner: the seam of the known ray.
pub fn seam(name: &str, place: &str, parts: Parts, c: V3, at: V3) -> Fixture {
    let at = c + at;
    let mut from = vec![at];
    for k in 0..3 {
        // A step of f32 at 64; beside 0 itself there are only the subnormal ones, which are ignored.
        for f in if at[k] == 0.0 { [-7.629_394_5e-6, 7.629_394_5e-6] } else { [at[k].next_down(), at[k].next_up()] } {
            from.push(V3(std::array::from_fn(|i| if i == k { f } else { at[i] })));
        }
    }
    Fixture { seam: true, ..Fixture::new(format!("from {name}, {place}"), parts, light(c), from, directions(512), any) }
}

/// Alpha exactly at a candidate's own weight, where the solver's test and weights decide: sheets `height` in front of
/// each point, their alpha a corner's weight where the ray meets them, the cutoff that weight (they stop the ray) or
/// the float above it (it goes on to the wall `behind` them). A weight a step off either way turns one of the two. At
/// 0.3 the sheets are in the points' lists; at 3, with the wall 0.005 behind, in none, and the window about what the
/// structure met (the sheet or the wall) decides.
fn thresholds(out: &mut Vec<Fixture>, how: &str, height: f32, behind: f32) -> Result<(), String> {
    let along = vec![unit(v(0.13, -0.21, -1.0))];
    let (mut meshes, mut from) = (Vec::new(), Vec::new());
    for i in 0..16 {
        let (c, s) = (v(40.0 * i as f32, 0.0, 0.0), 1.0 + 0.137 * i as f32);
        let corners = [c + v(-9.3 * s, -7.1, 0.0), c + v(8.7, -6.3 * s, 0.0), c + v(0.9 * s, 9.9, 0.0)];
        let colors = [0.0, 1.0, 0.0].map(|a| [1.0, 1.0, 1.0, a]).to_vec();
        meshes.push(Mesh { positions: corners.map(|p| p.0).to_vec(), indices: vec![0, 1, 2], colors, ..Mesh::default() });
        from.push(c + v(0.37 * s - 1.1, 0.53 * i as f32 - 2.0, height));
    }
    let wall = rectangle(v(300.0, 0.0, -behind), v(400.0, 0.0, 0.0), v(0.0, 40.0, 0.0), coloured(0.7, 0.8, 0.9));
    let sheet = |cutoff: f32| Material { cutoff, cull: Cull::None, ..coloured(0.9, 0.4, 0.2) };
    let with = |cutoffs: &[f32]| -> Parts { [wall.clone()].into_iter().chain(meshes.iter().zip(cutoffs).map(|(m, c)| (m.clone(), sheet(*c)))).collect() };
    // The CPU's weights where each ray meets its sheet, all of them opaque, and its alpha there.
    let probe = Fixture::new("", with(&[0.0; 16]), LightState::default(), Vec::new(), Vec::new(), any);
    let scene = prepare(probe.scene(&probe.parts), &SceneLimits::default(), 1, &|| false).map_err(|e| e.to_string())?;
    let e = scene.export();
    let mut at = Vec::new();
    for p in &from {
        let h = scene.transport(&Ray::new(*p, along[0]), f32::INFINITY).ok_or("a sheet that is not met")?;
        let (a, w) = (e.faces[h.tri() as usize].colors.map(|c| c[3]), h.weights());
        at.push(a[0] * w[0] + a[1] * w[1] + a[2] * w[2]);
    }
    let above: Vec<f32> = at.iter().map(|a| a.next_up()).collect();
    let light = one_lamp(v(300.0, 0.0, 60.0));
    let stops: Shows = |b| b.iter().all(|b| b.is_some_and(|b| b.tri >= 2));
    let goes_on: Shows = |b| b.iter().all(|b| b.is_some_and(|b| b.tri < 2));
    // The sheet or the wall itself, on every ray: the wall is in the sheet's place for a tie.
    out.push(Fixture { exact: true, ..Fixture::new(format!("alpha at a hit's own weight, {how}, the cutoff that weight"), with(&at), light.clone(), from.clone(), along.clone(), stops) });
    out.push(Fixture { exact: true, ..Fixture::new(format!("alpha at a hit's own weight, {how}, the cutoff the float above"), with(&above), light, from, along, goes_on) });
    Ok(())
}

/// Main's half brightness: a spot 0.005 above the place, its cone's own direction unit; 45 degrees off, nothing.
fn close_spot(out: &mut Vec<Fixture>) {
    let floor = rectangle(v(7.0, 0.0, 3.0), v(0.0, 0.0, 50.0), v(50.0, 0.0, 0.0), coloured(0.5, 0.6, 0.7));
    let spot = Lamp { cone: Some(Cone::new(v(0.0, -1.0, 0.0), 0.3, 0.5)), ..lamp(v(0.0, 0.005, 0.0), V3([0.8; 3]), 100.0) };
    let half: Shows = |b| b[0].is_some_and(|b| b.light.max_abs() > 0.01 && b.shadow_rays == 1) && b[1].is_some_and(|b| b.light == V3::ZERO && b.shadow_rays == 0);
    let light = LightState { ambient: V3([0.1; 3]), lamps: vec![spot], ..LightState::default() };
    out.push(Fixture::new("a spot 0.005 above the place, at half brightness", vec![floor.clone()], light, vec![v(0.0, 30.0, 0.0), v(0.005, 30.0, 0.0)], vec![v(0.0, -1.0, 0.0)], half));
    // A lamp of the least range packed, 0.3 above the floor and 0.58 from the hit, in the hit's cell of the lamps' grid
    // (which a far lamp makes 1.3 a side): out of range, no light and no shadow ray, though the place faces it.
    let tiny = LightState { ambient: V3([0.1; 3]), lamps: vec![lamp(v(0.2, 0.3, 0.1), V3([0.8; 3]), 1e-6), lamp(v(40.0, -20.0, 40.0), V3([0.8; 3]), 1.0)], ..LightState::default() };
    let out_of_range: Shows = |b| b[0].is_some_and(|b| b.lamps == 1 && b.shadow_rays == 0 && b.light == V3::ZERO);
    out.push(Fixture::new("a lamp of the least range in the cell of a hit 0.58 off", vec![floor], tiny, vec![v(0.5, 20.0, 0.5)], vec![v(0.0, -1.0, 0.0)], out_of_range));
}

/// Straight down at the faces of the lamps' grid, and at its far edge where a place is rounded onto it.
fn grid(out: &mut Vec<Fixture>) -> Result<(), String> {
    let floor = rectangle(V3::ZERO, v(0.0, 0.0, 1500.0), v(1500.0, 0.0, 0.0), coloured(0.6, 0.6, 0.6));
    let light = LightState { ambient: V3([0.1; 3]), lamps: vec![lamp(v(-300.0, 80.0, 10.0), V3([0.9; 3]), 400.0), lamp(v(250.0, 90.0, 0.0), v(0.2, 0.9, 0.5), 350.0), lamp(v(10.0, 120.0, -60.0), v(1.0, 0.1, 0.3), 200.0)], ..LightState::default() };
    let table = LampTable::of(&light, &|| false).map_err(|e| e.to_string())?;
    let mut from = Vec::new();
    for k in [0, 2] {
        for face in [0, 1, table.cells[k] / 2, table.cells[k] - 1, table.cells[k]] {
            let face = table.min[k] + face as f32 * table.side;
            for x in [face.next_down(), face, face.next_up(), face + 0.3] {
                from.push(if k == 0 { v(x, 40.0, 3.0) } else { v(12.0, 40.0, x) });
            }
        }
    }
    out.push(Fixture::new("the faces of the lamps' grid", vec![floor], light, from, vec![v(0.0, -1.0, 0.0)], |b| lit(b) && b.iter().flatten().any(|b| b.lamps == 0)));
    // Main's cases (light.rs `the_grid_at_its_edges`): the place's c is rounded to `cells` itself, and is the last cell's.
    // The place is about main's, on the axis through the lamps, where the near lamp's sphere still holds it (a room's: 1e-5
    // inside a range of 100), on a wall whose triangles' shared side is elsewhere.
    let edge: Shows = |b| b.iter().all(|b| b.is_some_and(|b| b.lamps > 0 && b.shadow_rays > 0 && b.light.max_abs() > 0.0));
    let lamps = |l: &[(f32, f32)]| LightState { ambient: V3([0.1; 3]), lamps: l.iter().map(|(x, range)| lamp(v(*x, 0.0, 0.0), V3([0.8; 3]), *range)).collect(), ..LightState::default() };
    let wall = |c: V3, half: f32| rectangle(c, v(0.0, 0.0, half), v(0.0, half, 0.0), coloured(0.6, 0.6, 0.6));
    let rooms = lamps(&[(-2900.3, 100.0), (-100.0, 100.0)]);
    let worlds = lamps(&[(-2_097_152.0, 1.0), (-786_491.0 - 0.8125, 60.01), (0.0, 1.0)]);
    for (name, light, wall, from) in [("a room's", rooms, wall(v(-1e-5, 7.0, 5.0), 30.0), v(-50.0, 0.001, 0.002)), ("a world's", worlds, wall(v(0.99, 0.0, 0.0), 0.5), v(0.2, 0.11, 0.07))] {
        let f = Fixture::new(format!("the lamps' grid's far edge, {name}"), vec![wall], light, vec![from], vec![v(1.0, 0.0, 0.0)], edge);
        // That the CPU's place is on the edge: c along x is the cells' number itself.
        let scene = prepare(f.scene(&f.parts), &SceneLimits::default(), 1, &|| false).map_err(|e| e.to_string())?;
        let h = scene.transport(&Ray::new(from, f.along[0]), f32::INFINITY).ok_or("the wall is not met")?;
        let t = LampTable::of(&f.light, &|| false).map_err(|e| e.to_string())?;
        let c = (h.p[0] - t.min[0]) / t.side;
        if c != t.cells[0] as f32 {
            return Err(format!("{}: the place {:?} is at {c} of {} cells", f.name, h.p.0, t.cells[0]));
        }
        out.push(f);
    }
    Ok(())
}

/// As the solver's shadow rays are lifted, and how far surfaces that are in one place are apart at most.
fn lift(p: V3) -> f32 {
    0.01f32.max(4e-6 * p.max_abs())
}

/// How a ray's answers differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Same,
    /// The same triangle, other light or another place on it.
    Light,
    Place,
    /// Another triangle in the very place, parallel to it: the same light, or not.
    TieSame,
    Tie,
    /// Another triangle at the same place, on the other side of an edge.
    Edge,
    /// Another triangle elsewhere, or a side.
    Other,
    OnlyCpu,
    OnlyGpu,
    /// The same answer, but not the same lamps or shadow rays.
    Counts,
}

fn kind(cpu: &Option<Bounce>, gpu: &Answer, order: &[u32], normals: &[V3]) -> Kind {
    let Some(c) = cpu else { return if gpu.hit { Kind::OnlyGpu } else { Kind::Same } };
    if !gpu.hit {
        return Kind::OnlyCpu;
    }
    let same = (c.light - gpu.light).max_abs() <= TOLERANCE && c.hidden == gpu.hidden;
    let close = (c.p - gpu.p).max_abs() <= lift(c.p);
    let Some(tri) = order.get(gpu.primitive as usize) else { return Kind::Other };
    if *tri == c.tri {
        return match (c.front == gpu.front, close && (c.start - gpu.start).max_abs() <= 2.0 * lift(c.p), same) {
            (false, ..) => Kind::Other,
            (_, false, _) => Kind::Place,
            (_, _, true) => Kind::Same,
            _ => Kind::Light,
        };
    }
    // Two surfaces in one place are met where the ray crosses it: the two places differ by the rounding of a distance,
    // a few steps of an f32, far below the lift.
    let (a, b) = (normals[c.tri as usize], normals[*tri as usize]);
    let parallel = a.dot(b).abs() >= 0.99 && a.dot(c.p - gpu.p).abs() <= (0.5 * lift(c.p)).min(0.02) && close;
    match (parallel, close, same) {
        (true, _, true) => Kind::TieSame,
        (true, ..) => Kind::Tie,
        (_, true, _) => Kind::Edge,
        _ => Kind::Other,
    }
}

/// The counts of an answer: a hit, a hidden one, its lamps and shadow rays.
fn counts(b: &Option<Bounce>) -> [u32; 4] {
    b.map_or([0; 4], |b| [1, u32::from(b.hidden), b.lamps, b.shadow_rays])
}

/// The light of a ray's triangle alone, from the same place along the same direction, in the light of each set of the
/// lamps: every lamp of the set in sight, so that its shadow rays are the lamps it sees. A seam's ray whose shadow rays
/// start in another surface's plane brings one of these.
fn in_sight(f: &Fixture, e: &Export, tri: u32, ray: (V3, V3)) -> Result<Vec<(V3, u32)>, String> {
    let face = &e.faces[tri as usize];
    let mesh = Mesh { positions: e.corners[tri as usize].map(|c| c.0).to_vec(), normals: face.normals.to_vec(), uv0: face.uv.to_vec(), colors: face.colors.to_vec(), indices: vec![0, 1, 2] };
    let input = SceneInput { meshes: vec![mesh], instances: vec![Instance { mesh: 0, material: face.material as usize, world_from_local: SAME }], materials: e.materials.to_vec(), textures: f.textures.clone() };
    let alone = prepare(input, &SceneLimits::default(), 1, &|| false).map_err(|e| e.to_string())?;
    let lamps = &f.light.lamps;
    let mut out = Vec::new();
    for set in 0..1usize << lamps.len().min(4) {
        let light = LightState { lamps: lamps.iter().enumerate().filter(|(i, _)| set >> i & 1 == 1).map(|(_, l)| *l).collect(), ..f.light.clone() };
        let b = bounces(&alone, &light, &[ray], EMISSION, 1, &|| false).map_err(|e| e.to_string())?;
        out.extend(b[0].map(|b| (b.light, b.shadow_rays)));
    }
    Ok(out)
}

/// Whether the shadow rays of the CPU's answer start in another surface's plane: within half the lift of a triangle
/// other than its own (its own plane's neighbours are the whole lift off).
fn starts_on_another(scene: &PreparedScene, c: &Bounce) -> bool {
    c.shadow_rays > 0 && scene.near(c.start, 0.5 * lift(c.p)).iter().any(|n| n.tri != c.tri)
}

/// A fixture's verdict, printed: whether it passed, and whether what differs is the known difference.
fn check(gpu: &Gpu, f: &Fixture) -> Result<(bool, bool), String> {
    let rays: Vec<(V3, V3)> = f.from.iter().flat_map(|from| f.along.iter().map(|d| (*from, *d))).collect();
    let of = |parts: &Parts, light: &LightState| {
        let scene = prepare(f.scene(parts), &SceneLimits::default(), 1, &|| false).map_err(|e| e.to_string())?;
        let cpu = bounces(&scene, light, &rays, EMISSION, 4, &|| false).map_err(|e| e.to_string())?;
        Ok::<_, String>((scene, cpu))
    };
    let (scene, cpu) = of(&f.parts, &f.light)?;
    let others = f.others.iter().map(|(parts, light, shadowed)| Ok((of(parts, light)?.1, *shadowed))).collect::<Result<Vec<_>, String>>()?;
    let tables = Tables::new(&scene, &f.along)?;
    let e = scene.export();
    let normals: Vec<V3> = e.faces.iter().map(|f| f.normal).collect();
    let order = tables.order();
    let points = Points::at(f.from.clone(), &scene, &tables)?;
    let room = gpu.room(&tables)?;
    let config = Config { emission: EMISSION, ..Config::default() };
    let answers = answers_of(&gpu.run(Target::Replay, &room, &points, &f.light, &config)?).into_iter().collect::<Option<Vec<_>>>().ok_or("a ray without an answer")?;
    let probes = probes_of(&gpu.run(Target::Moments, &room, &points, &f.light, &config)?);
    let reach = reach_of(&gpu.run(Target::Reach, &room, &points, &f.light, &config)?);
    let counted = classified(&scene, &f.from, &f.along, 4, &|| false).map_err(|e| e.to_string())?;
    let n = f.along.len();
    if answers.len() != rays.len() || probes.len() != f.from.len() || reach.len() != f.from.len() {
        return Err(format!("{} answers, {} probes, {} reaches for {} rays", answers.len(), probes.len(), reach.len(), rays.len()));
    }
    // Of each point: the sums of the answers its rays agreed with (their counts, their light's moments), whether any
    // was another than the scene's own, and what the known ray moves its hidden sides by.
    let (mut sums, mut moments, mut named) = (vec![[0u32; 4]; f.from.len()], vec![[V3::ZERO; 4]; f.from.len()], vec![false; f.from.len()]);
    let (mut off, mut alternatives, mut exempt, mut known, mut backs) = (Vec::new(), Vec::new(), Vec::new(), None, vec![0i64; f.from.len()]);
    for (i, (c, g)) in cpu.iter().zip(&answers).enumerate() {
        let finite = !g.hit || (g.light.finite() && g.p.finite() && g.start.finite() && g.t.is_finite() && g.weights.iter().all(|w| w.is_finite()));
        // Its shadow rays start off the face it met, on the side it met, as the solver lifts them.
        let lifted = order.get(g.primitive as usize).is_some_and(|t| {
            let n = if g.front { normals[*t as usize] } else { -normals[*t as usize] };
            (g.start - (g.p + n * lift(g.p))).max_abs() <= 1e-6 * (1.0 + g.p.max_abs())
        });
        let same_counts = |o: &Option<Bounce>| counts(o)[2..] == [g.lamps, g.shadow_rays];
        let mut k = kind(c, g, order, &normals);
        if matches!(k, Kind::Same | Kind::TieSame) && !same_counts(c) {
            k = Kind::Counts;
        }
        // From a seam, in sight of other lamps than the CPU's where its shadow rays start in another surface's plane: the
        // light and the lamps in sight of one set of the lamps.
        let mut seam = false;
        if let Some(b) = c.filter(|b| f.seam && finite && k == Kind::Light && b.hidden == g.hidden && starts_on_another(&scene, b)) {
            seam = in_sight(f, &e, b.tri, rays[i])?.iter().any(|(l, seen)| (*l - g.light).max_abs() <= TOLERANCE && *seen == g.free);
        }
        let own_light = seam && same_counts(c);
        if own_light {
            exempt.push((i / n, i % n));
        }
        let near_known = (g.light - V3(KNOWN.light)).max_abs() <= TOLERANCE && g.free == KNOWN.free;
        let pinned = f.name == KNOWN.fixture && i == KNOWN.ray && n == 512 && finite && g.hit && k == Kind::Edge && c.is_some_and(|c| c.tri == KNOWN.cpu && c.hidden) && order.get(g.primitive as usize) == Some(&KNOWN.gpu) && g.front && !g.hidden && near_known && lifted;
        let delta = c.map(|c| [0, -i64::from(c.hidden), i64::from(g.lamps) - i64::from(c.lamps), i64::from(g.shadow_rays) - i64::from(c.shadow_rays)]);
        let tie = k == Kind::TieSame && !f.exact && lifted;
        let (agreed, dark) = if finite && (k == Kind::Same || tie || own_light) {
            (Some(*c), false)
        } else if pinned && delta == Some(KNOWN.delta) {
            // The known ray: the point is held to the kernel's answer of it, every other ray to the CPU's.
            known = Some(i);
            backs[i / n] = KNOWN.delta[1];
            (Some(Some(Bounce { hidden: false, lamps: g.lamps, shadow_rays: g.shadow_rays, light: g.light, ..c.unwrap_or_else(|| unreachable!()) })), false)
        } else {
            let other = |(o, shadowed): (&Option<Bounce>, bool)| match o {
                None => !g.hit,
                Some(o) => {
                    let light = if shadowed { g.light == V3::ZERO && g.free == 0 } else { (o.light - g.light).max_abs() <= 4.0 * TOLERANCE };
                    let place = order.get(g.primitive as usize) == Some(&o.tri) && o.front == g.front && (o.p - g.p).max_abs() <= 0.02 && (o.start - g.start).max_abs() <= 2.0 * lift(o.p);
                    g.hit && o.hidden == g.hidden && light && place && same_counts(&Some(*o))
                }
            };
            let found = others.iter().enumerate().map(|(j, (o, shadowed))| (j, &o[i], *shadowed)).find(|(_, o, shadowed)| finite && other((o, *shadowed)));
            if let Some((j, ..)) = found {
                alternatives.push((i / n, i % n, j));
                named[i / n] = true;
            }
            found.map_or((None, false), |(_, o, shadowed)| (Some(*o), shadowed))
        };
        if agreed.is_none() {
            off.push((i, k));
        }
        let used = agreed.unwrap_or(*c);
        let [a, b, l, s] = counts(&used);
        sums[i / n] = [sums[i / n][0] + a, sums[i / n][1] + b, sums[i / n][2] + l, sums[i / n][3] + s];
        // The moments take the light of the answer agreed with: none of one in shadow, the kernel's own where an
        // exemption bounded it.
        let l = if own_light { g.light } else { used.filter(|u| !u.hidden && !dark).map_or(V3::ZERO, |u| u.light) };
        let (d, m) = (f.along[i % n], &mut moments[i / n]);
        *m = [m[0] + l, m[1] + l * (2.0 * d[0]), m[2] + l * (2.0 * d[1]), m[3] + l * (2.0 * d[2])];
    }
    // A point is off where its moments are no numbers, its counts are not the sums, a moment of its light is not
    // theirs (C0 within the tolerance, the bands, of twice the light, within twice it), or its reach is not the CPU's
    // (moved by the known ray alone, where it is).
    let tolerances = [TOLERANCE, 2.0 * TOLERANCE, 2.0 * TOLERANCE, 2.0 * TOLERANCE];
    let mut points_off = Vec::new();
    for (k, (p, r)) in probes.iter().zip(&reach).enumerate() {
        let cpu_reach = if named[k] { [sums[k][0], sums[k][1]] } else { [counted[k][0], (i64::from(counted[k][1]) + backs[k]) as u32] };
        let main = cpu[k * n..(k + 1) * n].iter().fold([0; 2], |s, b| [s[0] + counts(b)[0], s[1] + counts(b)[1]]);
        let mean = moments[k].map(|m| m * (1.0 / n as f32));
        let apart: [f32; 4] = std::array::from_fn(|j| (p.moments.c[j] - mean[j]).max_abs());
        let numbers = p.moments.c.iter().all(|c| c.finite());
        if !numbers || [p.hits, p.backs, p.lamps, p.shadow_rays] != sums[k] || (0..4).any(|j| apart[j] > tolerances[j]) || *r != cpu_reach || counted[k] != main {
            points_off.push((k, mean, apart));
        }
    }
    let shows = (f.shows)(&cpu);
    let passed = off.is_empty() && points_off.is_empty() && shows;
    let verdict = match (passed, known) {
        (true, Some(_)) => "KNOWN",
        (true, None) => "PASS",
        _ => "FAIL",
    };
    let met = [cpu.iter().flatten().count(), answers.iter().filter(|g| g.hit).count()];
    println!("{verdict} {}: {} rays, {} differ, {} another answer that is named, {} from a seam in sight of other lamps; hits {} / {}; {} points, {} off{}", f.name, cpu.len(), off.len(), alternatives.len(), exempt.len(), met[0], met[1], f.from.len(), points_off.len(), if shows { "" } else { "; the CPU's answers do not show what it is for" });
    if !alternatives.is_empty() {
        println!("    named answers (point, direction, which): {alternatives:?}");
    }
    if !exempt.is_empty() {
        println!("    from a seam, in sight of other lamps (point, direction): {exempt:?}");
    }
    if known.is_some() {
        println!("    known: ray {}, {}", KNOWN.ray, KNOWN.why);
    } else if passed && f.name == KNOWN.fixture {
        println!("    the known ray agrees now");
    }
    for (i, k) in off.iter().take(4) {
        println!("    ray {i} from {:?} along {:?}: {k:?}", f.from[i / n].0, f.along[i % n].0);
        println!("      CPU {:?}", cpu[*i]);
        println!("      GPU {:?}", answers[*i]);
    }
    for (k, mean, apart) in points_off.iter().take(4) {
        let p = &probes[*k];
        println!("    point {k} at {:?}: GPU hits, backs, lamps, shadow rays {:?}, reach {:?}; CPU {:?}, reach {:?}", f.from[*k].0, [p.hits, p.backs, p.lamps, p.shadow_rays], reach[*k], sums[*k], counted[*k]);
        for (j, name) in ["C0", "Cx", "Cy", "Cz"].iter().enumerate() {
            println!("      {name}: GPU {:?}, CPU {:?}, off by {} (tolerance {})", p.moments.c[j].0, mean[j].0, apart[j], tolerances[j]);
        }
    }
    Ok((passed, known.is_some()))
}

/// Whether two f32 are one number: the same bits, or 0 and -0. The second is counted.
fn one(a: f32, b: f32, zeros: &mut usize) -> bool {
    let signed = a.to_bits() != b.to_bits() && a == 0.0 && b == 0.0;
    *zeros += usize::from(signed);
    a.to_bits() == b.to_bits() || signed
}

/// The kernel's triangle test against the solver's: random triangles, some aligned to the axes with corners on a
/// lattice, rays through the middle, the edges and the corners, from near and far and from the plane itself, and near
/// misses. Distances and weights the same bits (0 and -0 are one: the kernel's quotient of a 0 is 0), nothing met by
/// one alone.
fn triangle_test(gpu: &Gpu) -> Result<bool, String> {
    let mut state = 3009u64;
    let mut draw = move || {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 40) as f32 / 16_777_216.0
    };
    let mut along = directions(1024);
    along.extend((0..6).map(|i| V3(std::array::from_fn(|k| if k == i / 2 { 1.0 - 2.0 * (i % 2) as f32 } else { 0.0 }))));
    let (n, mut mesh, mut from) = (100_000usize, Mesh::default(), Vec::new());
    while from.len() < n {
        let i = from.len();
        let middle = [V3::ZERO, v(1700.0, 7500.0, -800.0), v(7936.0, 8000.0, -1792.0)][i % 3];
        let size = [1.0, 40.0, 400.0][i / 3 % 3];
        let lattice = (i / 9).is_multiple_of(2);
        let mut corner = || {
            let p = v(draw() - 0.5, draw() - 0.5, draw() - 0.5) * size;
            let p = if lattice { V3(p.0.map(|c| (c * 4.0 / size).round() * size / 4.0)) } else { p };
            middle + p
        };
        let t = [corner(), corner(), corner()];
        let (x, y) = (t[1] - t[0], t[2] - t[0]);
        // What is left out of a scene would shift the numbers.
        if x.unit().zip(y.unit()).and_then(|(x, y)| x.cross(y).unit()).is_none() {
            continue;
        }
        let w = match i / 18 % 5 {
            0 => [0.25 + 0.5 * draw(), 0.2, 0.0],
            1 => [draw(), 1.0, 0.0],
            2 => [1.0, 0.0, 0.0],
            3 => [0.5, 0.5, 0.0],
            _ => [draw() * 1.0001, -1e-6, 0.0],
        };
        let target = t[0] * w[0] + t[1] * (w[1] * (1.0 - w[0])) + t[2] * (1.0 - w[0] - w[1] * (1.0 - w[0]));
        let far = [0.0, 1e-3, 3.0, 300.0][i / 90 % 4] * (0.5 + draw());
        mesh.indices.extend((0..3).map(|k| (mesh.positions.len() + k) as u32));
        mesh.positions.extend(t.map(|p| p.0));
        from.push(target - along[i % along.len()] * far);
    }
    let input = SceneInput { meshes: vec![mesh], instances: vec![Instance { mesh: 0, material: 0, world_from_local: SAME }], materials: vec![Material::default()], ..SceneInput::default() };
    let scene = prepare(input, &SceneLimits::default(), 4, &|| false).map_err(|e| e.to_string())?;
    let tables = Tables::new(&scene, &along)?;
    let room = gpu.room(&tables)?;
    let gpu = meetings_of(&gpu.run(Target::Meetings, &room, &Points::bare(from.clone()), &LightState::default(), &Config::default())?);
    let corners = scene.export().corners;
    if corners.len() != n || gpu.len() != n || tables.order().iter().enumerate().any(|(i, t)| *t as usize != i) {
        return Err(format!("{} triangles, {} answers, {n} expected, in the scene's order", corners.len(), gpu.len()));
    }
    let (mut met, mut equal, mut weights, mut other, mut zeros, mut first) = (0, 0, 0, 0, 0, Vec::new());
    for (i, ((from, corners), gpu)) in from.iter().zip(&corners).zip(&gpu).enumerate() {
        match (meet(*from, along[i % along.len()], corners), gpu.met) {
            (None, None) => {}
            (Some((a, x)), Some((b, y))) => {
                met += 1;
                let same = [one(a, b, &mut zeros), x.iter().zip(y).all(|(x, y)| one(*x, y, &mut zeros))];
                equal += usize::from(same[0]);
                weights += usize::from(same[1]);
                if same != [true; 2] && first.len() < 6 {
                    first.push((i, a, b));
                }
            }
            (a, b) => {
                other += 1;
                if first.len() < 6 {
                    first.push((i, a.map_or(f32::NAN, |a| a.0), b.map_or(f32::NAN, |b| b.0)));
                }
            }
        }
    }
    let passed = equal == met && weights == met && other == 0;
    let listed = if first.is_empty() { String::new() } else { format!("; {first:?}") };
    println!("{} the triangle test: {n} pairs, {met} met by both, {equal} distances and {weights} weights the same bits ({zeros} numbers 0 on one side and -0 on the other, which are one), {other} met by one alone{listed}", if passed { "PASS" } else { "FAIL" });
    Ok(passed)
}

/// Whether all that has to agree does; what is known is told apart.
pub fn run(gpu: &Gpu) -> Result<bool, String> {
    let mut all = Vec::new();
    surfaces(&mut all);
    thresholds(&mut all, "in the points' lists", 0.3, 5.0)?;
    thresholds(&mut all, "decided in the window", 3.0, 0.005)?;
    close_spot(&mut all);
    grid(&mut all)?;
    all.push(seam("the edge of two walls", PLACES[0].0, room(V3::ZERO, V3([128.0; 3])), V3::ZERO, v(128.0, 128.0, 10.0)));
    crate::corpus::add(&mut all);
    let (mut failed, mut known) = (0, 0);
    for f in &all {
        let (passed, is_known) = check(gpu, f).map_err(|e| format!("{}: {e}", f.name))?;
        failed += usize::from(!passed);
        known += usize::from(is_known);
    }
    let test = triangle_test(gpu)?;
    println!("fixtures: {} of {} pass, {known} of them by the known difference, {failed} fail; the triangle test {}", all.len() - failed, all.len(), if test { "passes" } else { "FAILS" });
    Ok(failed == 0 && test)
}
