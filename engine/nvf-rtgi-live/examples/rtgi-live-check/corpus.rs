//! The larger set of the check: outgoing light far from the origin, a shared edge, walls, seams, ties, shadows' ends
//! and sheets, each with the answers it names beside it.

use crate::fixtures::{any, coloured, light, lit, one_lamp, rectangle, room, seam, unit, v, Fixture, Parts, Shows, PLACES};
use nvf_rtgi::{directions, Bounce, Cull, Level, LightState, Material, Mesh, Texture, Wrap, V3};

fn dark(b: &[Option<Bounce>]) -> bool {
    b.iter().all(|b| b.is_some_and(|b| b.light == V3::ZERO))
}

/// One alpha all over.
fn flat(alpha: u8) -> Texture {
    Texture { levels: vec![Level { width: 2, height: 2, texels: vec![[255, 255, 255, alpha]; 4] }], srgb: false, wrap: [Wrap::Repeat; 2] }
}

fn surfaces(out: &mut Vec<Fixture>) {
    let shows: Shows = |b| b.iter().flatten().any(|b| b.shadow_rays > 0 && b.light[0] > 0.2) && b.iter().flatten().any(|b| b.lamps == 0 && b.light.max_abs() > 0.01);
    let c = v(60_000.0, 2_000.0, -40_000.0);
    out.push(Fixture::new("outgoing light, far from the origin", room(c, v(100.0, 90.0, 120.0)), light(c), vec![c + v(7.3, -11.1, 3.7), c + v(-61.7, 43.9, 22.2)], directions(512), shows));
    let floor = || rectangle(V3::ZERO, v(0.0, 0.0, 64.0), v(64.0, 0.0, 0.0), coloured(0.5, 0.6, 0.7));
    let from = v(10.0, 50.0, -7.0);
    // Along its diagonal, and at its ends but for the corners.
    let diagonal: Vec<V3> = (1..64).map(|i| unit(v(2.0 * i as f32 - 64.0, 0.0, 2.0 * i as f32 - 64.0) - from)).collect();
    out.push(Fixture::new("a shared edge", vec![floor()], one_lamp(v(0.0, 60.0, 0.0)), vec![from], diagonal, |b| b.iter().all(|b| b.is_some()) && lit(b)));
    let decal = (floor().0, coloured(0.9, 0.1, 0.1));
    let down: Vec<V3> = directions(64).into_iter().filter(|d| d[1] < -0.75).collect();
    out.push(Fixture::new("two surfaces in one place", vec![floor(), decal], one_lamp(v(0.0, 60.0, 0.0)), vec![from], down, lit));
}

/// From a wall, a float to either side of it, and a little further.
fn walls(out: &mut Vec<Fixture>) {
    for (place, c) in PLACES {
        let wall = c[0] + 128.0;
        for (name, x, near) in [
            ("on a wall", wall, true),
            ("one float inside a wall", wall.next_down(), true),
            ("one float behind a wall", wall.next_up(), true),
            ("a little inside a wall", wall - 0.05, false),
            ("a little behind a wall", wall + 0.05, false),
        ] {
            let from = vec![v(x, c[1] + 64.0, c[2]), v(x, c[1] - 64.0, c[2] + 64.0)];
            let along = directions(512).into_iter().filter(|d| near || d[0].abs() > 0.05).collect();
            out.push(Fixture::new(format!("{name}, {place}"), room(c, V3([128.0; 3])), light(c), from, along, any));
        }
    }
}

/// From where surfaces meet, exactly there and a float or so beside: an edge and a corner of two and three walls, the seam
/// of two floors in one plane, the edge of a floor's two triangles, a corner of eight, a floor's edge with nothing beyond.
/// Their rays' shadow rays may start in another surface's plane (`Fixture::seam`). The edge of two walls at the origin
/// is the core set's.
fn seams(out: &mut Vec<Fixture>) {
    let walls = |c: V3| room(c, V3([128.0; 3]));
    let floors = |c: V3| [-64.0, 64.0].map(|z| rectangle(c + v(0.0, 0.0, z), v(0.0, 0.0, 64.0), v(64.0, 0.0, 0.0), coloured(0.5, 0.6 + z / 400.0, 0.7))).to_vec();
    let fan = |c: V3| {
        let ring = [[60.0, 0.0], [40.0, 45.0], [0.0, 55.0], [-35.0, 50.0], [-60.0, 5.0], [-45.0, -30.0], [0.0, -70.0], [50.0, -35.0]];
        let mut positions = vec![c.0];
        positions.extend(ring.map(|[x, z]| (c + v(x, 0.0, z)).0));
        let indices = (0..8u32).flat_map(|i| [0, 1 + (i + 1) % 8, 1 + i]).collect();
        let colors = (0..9).map(|i| [0.3 + 0.07 * i as f32, 0.5, 0.9 - 0.05 * i as f32, 1.0]).collect();
        vec![(Mesh { positions, indices, colors, ..Mesh::default() }, Material::default())]
    };
    for (place, c) in PLACES {
        let cases: [(&str, Parts, V3); 7] = [
            ("the edge of two walls", walls(c), v(128.0, 128.0, 10.0)),
            ("the corner of three walls", walls(c), V3([128.0; 3])),
            ("the edge of a wall's triangles", walls(c), v(128.0, 32.0, 32.0)),
            ("the seam of two floors", floors(c), v(10.0, 0.0, 0.0)),
            ("the corner of two floors", floors(c), v(64.0, 0.0, 0.0)),
            ("a floor's edge", floors(c), v(64.0, 0.0, 30.0)),
            ("the corner of eight triangles", fan(c), V3::ZERO),
        ];
        for (name, parts, at) in cases.into_iter().filter(|(name, ..)| c != V3::ZERO || *name != "the edge of two walls") {
            out.push(seam(name, place, parts, c, at));
        }
    }
}

/// Surfaces in one place: what the solver's rounding picks.
fn ties(out: &mut Vec<Fixture>) {
    let down: Vec<V3> = directions(256).into_iter().filter(|d| d[1] < -0.75).collect();
    for (place, c) in PLACES {
        // Not along an axis: the distances are not the same numbers.
        let (u, w) = (v(63.1, 7.7, 3.3), v(-4.1, 9.3, 61.7));
        let floor = |turned: bool, material| {
            let mut part = rectangle(c, w, u, material);
            if turned {
                // The same corners in another order, and the other winding.
                part.0.indices = vec![1, 0, 3, 1, 3, 2];
            }
            part
        };
        let front = coloured(0.5, 0.6, 0.7);
        let back = Material { cull: Cull::Back, ..coloured(0.9, 0.2, 0.2) };
        let cut = Material { texture: Some(0), cutoff: 0.5, cull: Cull::None, ..coloured(0.9, 0.1, 0.1) };
        for (name, a, b) in [
            ("a front and a back", floor(false, front), floor(true, back)),
            ("a back and a front", floor(true, back), floor(false, front)),
            ("two of one side", floor(false, front), floor(false, coloured(0.9, 0.1, 0.1))),
            ("a cutout and a surface", floor(true, cut), floor(false, front)),
            ("a surface and a cutout", floor(false, front), floor(true, cut)),
        ] {
            let from = vec![c + v(10.0, 50.0, -7.0), c + v(-3.3, 31.7, 8.1)];
            let f = Fixture::new(format!("{name} in one place, {place}"), vec![a, b], one_lamp(c + v(0.0, 60.0, 0.0)), from, down.clone(), any);
            out.push(Fixture { textures: vec![Some(flat(255))], ..f });
        }
        let decal = rectangle(c + v(0.0, 0.05, 0.0), v(0.0, 0.0, 20.0), v(20.0, 0.0, 0.0), coloured(0.9, 0.1, 0.1));
        let wall = rectangle(c, v(0.0, 0.0, 64.0), v(64.0, 0.0, 0.0), front);
        let decals: Shows = |b| b.iter().all(|b| b.is_some_and(|b| b.tri >= 2 && !b.hidden)) && lit(b);
        let low: Vec<V3> = (0..8).map(|i| unit(v(60.0, -1.2, i as f32 - 4.0))).collect();
        out.push(Fixture::new(format!("a decal, grazing, {place}"), vec![wall, decal], one_lamp(c + v(0.0, 60.0, 0.0)), vec![c + v(-60.0, 1.25, 0.0)], low, decals));
    }
}

/// A lamp above a floor with something between them: near either end of the shadow ray. At the end itself the
/// structure decides: the answers of the same scene with the thing 0.005 lower and higher are named.
fn shadows(out: &mut Vec<Fixture>) {
    let cut = Material { texture: Some(0), cutoff: 0.5, cull: Cull::None, ..Material::default() };
    let plain = Material { cull: Cull::None, ..Material::default() };
    for (kind, material) in [("opaque", plain), ("cutout", cut)] {
        // The lamp at 100 is drawn in by 0.01, the floor's end starts at 0.01.
        for (y, edge, shows) in [(99.98, false, dark as Shows), (99.99, true, any), (99.995, false, lit), (100.0, false, lit), (100.5, false, lit), (0.02, false, dark), (0.01, true, any), (0.005, false, lit)] {
            // The ray comes down at 45 degrees from +x, and meets what is above the floor at x = y: past its end.
            let with = |y: f32| {
                let middle = if y > 50.0 { v(0.0, y, 0.0) } else { v(y / 2.0 - 20.0, y, 0.0) };
                let floor = rectangle(V3::ZERO, v(0.0, 0.0, 90.0), v(70.0, 0.0, 0.0), coloured(0.5, 0.6, 0.7));
                vec![floor, rectangle(middle, v(0.0, 0.0, 20.0), v(20.0, 0.0, 0.0), material)]
            };
            let lamp = one_lamp(v(0.0, 100.0, 0.0));
            let others = if edge { [with(y - 5e-3), with(y + 5e-3)].map(|parts| (parts, lamp.clone(), false)).to_vec() } else { Vec::new() };
            let f = Fixture::new(format!("a shadow's end, {kind} at {y}"), with(y), lamp, vec![v(30.0, 30.0, 0.0)], vec![unit(v(-1.0, -1.0, 0.0))], shows);
            out.push(Fixture { others, textures: vec![Some(flat(255))], ..f });
        }
    }
    let floor = rectangle(V3::ZERO, v(0.0, 0.0, 90.0), v(70.0, 0.0, 0.0), coloured(0.5, 0.6, 0.7));
    out.push(Fixture::new("a lamp nearer than a shadow ray's ends", vec![floor], one_lamp(v(0.0, 0.015, 0.0)), vec![v(30.0, 30.0, 0.0)], vec![unit(v(-1.0, -1.0, 0.0))], lit));
}

/// A sheet before a wall, the lamp in front of both: from in front, and from between them.
fn sheets(out: &mut Vec<Fixture>) {
    let wall = || rectangle(v(0.0, 0.0, -60.0), v(90.0, 0.0, 0.0), v(0.0, 90.0, 0.0), coloured(0.7, 0.8, 0.9));
    // Both ways.
    let along: Vec<V3> = directions(256).into_iter().filter(|d| d[2].abs() > 0.8).collect();
    let before = one_lamp(v(5.0, 8.0, 50.0));
    let parts = |material: Material, alpha: f32| {
        let mut sheet = rectangle(V3::ZERO, v(90.0, 0.0, 0.0), v(0.0, 90.0, 0.0), material);
        sheet.0.colors = vec![[1.0, 1.0, 1.0, alpha]; 4];
        vec![wall(), sheet]
    };
    let mut add = |name: &str, others: Vec<(Parts, LightState, bool)>, material: Material, alpha: f32, textures: Vec<Option<Texture>>, shows| {
        let f = Fixture::new(name, parts(material, alpha), before.clone(), vec![v(1.0, 2.0, 40.0), v(1.5, -2.5, -30.0)], along.clone(), shows);
        out.push(Fixture { others, textures, ..f });
    };
    let through: Shows = |b| b.iter().flatten().any(|b| b.tri < 2 && b.light.max_abs() > 0.01);
    let stopped: Shows = |b| b.iter().flatten().all(|b| b.tri >= 2 || b.light == V3::ZERO) && b.iter().flatten().any(|b| b.tri >= 2);
    // With its back's normal turned, so that the lamp beyond it does not light it, whatever its own shadow.
    let cut = |alpha: f32| Material { double_sided: true, cutoff: 0.5, cull: Cull::None, base: [1.0, 0.5, 0.5, alpha], ..Material::default() };
    add("alpha below the cutoff", Vec::new(), cut(0.499), 1.0, Vec::new(), through);
    // Far from the points (none of them has the sheet in its list) the structure's barycentrics decide, the exact
    // weights only near a point and about the structure's nearest hit: a ray stopped or through it, and of one through
    // it, its shadow ray to the lamp stopped by it or not.
    let others = vec![(parts(cut(0.499), 1.0), before.clone(), false), (parts(cut(0.501), 1.0), before.clone(), false), (parts(cut(0.499), 1.0), before.clone(), true)];
    add("alpha at the cutoff", others, cut(0.5), 1.0, Vec::new(), any);
    add("alpha above the cutoff", Vec::new(), cut(0.501), 1.0, Vec::new(), stopped);
    let textured = |alpha| Material { texture: Some(0), ..cut(alpha) };
    add("a texture's alpha below the cutoff", Vec::new(), textured(1.0), 1.0, vec![Some(flat(127))], through);
    add("a texture's alpha above the cutoff", Vec::new(), textured(1.0), 1.0, vec![Some(flat(128))], stopped);
    // Infinite times the texel's 0.
    add("an alpha that is no number", Vec::new(), textured(1e3), 3e38, vec![Some(flat(0))], stopped);
    let sided = |cull, double_sided| Material { cull, double_sided, ..coloured(0.9, 0.4, 0.2) };
    let hidden: Shows = |b| b.iter().flatten().any(|b| b.hidden) && b.iter().flatten().all(|b| !b.hidden || (b.light == V3::ZERO && b.shadow_rays == 0));
    add("a hidden back", Vec::new(), sided(Cull::Back, false), 1.0, Vec::new(), hidden);
    add("a hidden front", Vec::new(), sided(Cull::Front, false), 1.0, Vec::new(), hidden);
    add("a double-sided sheet", Vec::new(), sided(Cull::None, true), 1.0, Vec::new(), stopped);
    add("a sheet drawn on both sides, its normal not turned", Vec::new(), sided(Cull::None, false), 1.0, Vec::new(), stopped);
}

pub fn add(out: &mut Vec<Fixture>) {
    surfaces(out);
    walls(out);
    seams(out);
    ties(out);
    shadows(out);
    sheets(out);
}
