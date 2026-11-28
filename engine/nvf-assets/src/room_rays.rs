//! Fit beam geometry to a model's light-ray shapes [design, #4698;
//! docs/design/room-beams.md].
//!
//! A ray shape is unlit (BSShaderNoLightingProperty), added to the frame and sets
//! BSShaderFlags bit 29 "External_Emittance" [documented: nif.xml flag names; the same
//! properties the renderer reads, `nif_spawn::shape_props`]. Never told by a file name.
//!
//! The room ray models are fans of flat cards swept along one direction: `model_glow`'s
//! principal axis does not suit them (their cards are taller than a third of their length),
//! so the axis here is the direction most of the card edges share, taken over the edges
//! along which the vertex alpha changes (a card's window edge keeps one alpha, its sides
//! fade) [observed: indfxlightrays01, indfxlightraysleft01 and indfxlightraysnleftsm01 of
//! `Fallout - Meshes.bsa`, their vertices read 2026-11-24].

use crate::data::GameData;
use crate::nif_spawn::{clean_vertex_color, shaded_model, shape_alpha_mode, shape_props};
use bevy::math::{Vec2, Vec3};
use bevy::prelude::AlphaMode;
use fnv_nif::scene::{scene_objects, triangle_indices, Decoded, SceneObject, SHAPE_TYPES};
use fnv_nif::NifFile;
use std::collections::HashMap;

/// Edges within this of one direction share it (cosine of 2 degrees) [design].
const SAME_DIRECTION: f32 = 0.999_39;
/// Vertex alphas at least this far apart count as a change [design].
const ALPHA_STEP: f32 = 0.02;
/// Vertices within this share of the beam's extent of each other across the axis are on one
/// line [design].
const LINE_SHARE: f32 = 0.02;
/// A section thinner than this share of its width is a slab (a flat patch drawn as stacked
/// cards), not a beam [design: the ray models' sections are 0.15-0.79, the floor spot's 0.04;
/// round 0's census, 2026-11-26].
pub const LEAST_ASPECT: f32 = 0.1;
/// A shape is long when its beam is this many times its mean width [design: `model_glow`'s
/// LONG].
pub const LONG: f32 = 1.5;

/// One ray shape's beam, in the shape's own space (game units before the reference's scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayBeam {
    /// The middle of the bright end (on the end's plane).
    pub a: Vec3,
    /// The beam's direction, from the bright end to the far one (unit).
    pub axis: Vec3,
    pub length: f32,
    /// The section's own axes (unit, square to the axis; `u` x `v` = axis).
    pub u: Vec3,
    pub v: Vec3,
    /// Half widths along `u` and `v` at the bright end and at the far one.
    pub half_a: [f32; 2],
    pub half_b: [f32; 2],
    /// How much the ends' planes lean: the distance along the axis per unit of `u` and `v`
    /// (zero: square to it).
    pub lean: [f32; 2],
    /// The share of the edges' length lying along the axis (0..1).
    pub share: f32,
}

impl RayBeam {
    /// The mean width: the side of a square as large as the mean section.
    pub fn width(&self) -> f32 {
        let area = |h: [f32; 2]| 4.0 * h[0] * h[1];
        (0.5 * (area(self.half_a) + area(self.half_b))).max(0.0).sqrt()
    }

    pub fn is_long(&self) -> bool {
        self.length >= LONG * self.width()
    }
}

/// One ray shape of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct RayShape {
    /// The shape's block index in its NIF: its identity (names repeat or are missing), the same
    /// as `model::PreparedShape::block`.
    pub block: u32,
    pub name: Option<String>,
    /// Its beam, or why none could be told (for the census log, round 0).
    pub beam: Result<RayBeam, &'static str>,
    /// Its shafts ([`ray_shafts`]): one beam a group of cards, empty when its cards make no clear
    /// groups (the whole beam then stands alone).
    pub shafts: Vec<RayBeam>,
}

#[cfg(test)]
fn ray_beam(verts: &[Vec3], tris: &[[u32; 3]], alphas: Option<&[f32]>) -> Option<RayBeam> {
    ray_beam_why(verts, tris, alphas).ok()
}

/// [`ray_beam`], or the reason it tells none (words for the census log) [design].
fn ray_beam_why(verts: &[Vec3], tris: &[[u32; 3]], alphas: Option<&[f32]>) -> Result<RayBeam, &'static str> {
    if verts.len() < 4 {
        return Err("fewer than 4 vertices");
    }
    if verts.iter().any(|p| !p.is_finite()) {
        return Err("a vertex is not a number");
    }
    let alphas = alphas.filter(|a| a.len() == verts.len());
    let mut pairs: Vec<(usize, usize)> = tris
        .iter()
        .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
        .map(|(a, b)| (a.min(b) as usize, a.max(b) as usize))
        .filter(|&(a, b)| a != b && b < verts.len())
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    let fades = |&(a, b): &(usize, usize)| alphas.is_some_and(|c| (c[a] - c[b]).abs() > ALPHA_STEP);
    let faded = pairs.iter().any(fades);
    if faded {
        pairs.retain(fades);
    }
    // Directions and lengths; the direction the most length shares.
    let edges: Vec<(Vec3, f32)> = pairs
        .iter()
        .map(|&(a, b)| verts[b] - verts[a])
        .filter(|e| e.length() > 1e-3)
        .map(|e| (e.normalize(), e.length()))
        .collect();
    let total: f32 = edges.iter().map(|e| e.1).sum();
    let score = |d: Vec3| -> f32 { edges.iter().filter(|e| e.0.dot(d).abs() > SAME_DIRECTION).map(|e| e.1).sum() };
    let (best, shared) = edges.iter().map(|e| (e.0, score(e.0))).max_by(|x, y| x.1.total_cmp(&y.1)).ok_or("no edges")?;
    let share = shared / total;
    let axis = edges
        .iter()
        .filter(|e| e.0.dot(best).abs() > SAME_DIRECTION)
        .fold(Vec3::ZERO, |s, e| s + e.0 * (e.1 * e.0.dot(best).signum()))
        .normalize_or_zero();
    if axis == Vec3::ZERO {
        return Err("no shared edge direction");
    }
    let along = |axis: Vec3| -> (Vec<f32>, f32, f32) {
        let t: Vec<f32> = verts.iter().map(|p| p.dot(axis)).collect();
        let (lo, hi) = t.iter().fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
        (t, lo, hi)
    };
    // The bright end: the alpha-weighted middle lies toward it.
    let (t, lo, hi) = along(axis);
    let weight: f32 = alphas.map_or(0.0, |a| a.iter().sum());
    let off = alphas
        .filter(|_| weight > 0.0)
        .map_or(0.0, |a| a.iter().zip(&t).map(|(a, t)| a * t).sum::<f32>() / weight - 0.5 * (lo + hi));
    let directed = off.abs() > 0.1 * (hi - lo);
    let axis = if directed && off > 0.0 { -axis } else { axis };
    let (t, lo, hi) = along(axis);
    let extent = hi - lo;
    if extent <= 1e-3 {
        return Err("no length along its axis");
    }
    // The section's axes: the widest spread across the axis, and square to it.
    let mean = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;
    let square = |d: Vec3| d - axis * d.dot(axis);
    let across: Vec<Vec3> = verts.iter().map(|p| square(*p - mean)).collect();
    let widest = across.iter().copied().max_by(|x, y| x.length_squared().total_cmp(&y.length_squared())).ok_or("no vertices")?;
    if widest.length() < 1e-3 {
        return Err("a line: no spread across its axis");
    }
    let mut u = widest.normalize();
    for _ in 0..64 {
        let next = square(across.iter().fold(Vec3::ZERO, |s, p| s + *p * p.dot(u)));
        if next.length_squared() < 1e-12 {
            break;
        }
        u = next.normalize();
    }
    let v = axis.cross(u);
    let flat: Vec<[f32; 2]> = across.iter().map(|p| [p.dot(u), p.dot(v)]).collect();
    // The lines along the axis: where each starts, and how long it is.
    let near = LINE_SHARE * extent;
    let mut used = vec![false; verts.len()];
    let (mut starts, mut lengths, mut on_lines) = (Vec::new(), Vec::new(), 0usize);
    for i in 0..verts.len() {
        if used[i] {
            continue;
        }
        let group: Vec<usize> = (i..verts.len()).filter(|&j| !used[j] && (flat[j][0] - flat[i][0]).hypot(flat[j][1] - flat[i][1]) < near).collect();
        for &j in &group {
            used[j] = true;
        }
        let (l, h) = group.iter().fold((f32::MAX, f32::MIN), |(l, h), &j| (l.min(t[j]), h.max(t[j])));
        if group.len() > 1 && h - l > 0.25 * extent {
            starts.push((flat[i], l));
            lengths.push(h - l);
            on_lines += group.len();
        }
    }
    lengths.sort_unstable_by(f32::total_cmp);
    let lined = starts.len() >= 3 && 2 * on_lines >= verts.len();
    let (start, lean, length) = match fit_plane(&starts).filter(|_| lined) {
        Some((c, s)) => (c, s, lengths[lengths.len() / 2]),
        None => (lo, [0.0; 2], extent),
    };
    // The section at each end, centred on the middle of the whole section.
    let range = |k: usize| flat.iter().fold((f32::MAX, f32::MIN), |(l, h), f| (l.min(f[k]), h.max(f[k])));
    let mid = [0, 1].map(|k| 0.5 * (range(k).0 + range(k).1));
    let tilt = |i: usize| (t[i] - start - lean[0] * flat[i][0] - lean[1] * flat[i][1]) / length;
    let section = |from: f32, to: f32| {
        let points: Vec<_> = flat.iter().enumerate()
            .filter(|(i, _)| (from..=to).contains(&tilt(*i)))
            .map(|(_, p)| Vec2::from_array(*p))
            .collect();
        let first = *points.first()?;
        let (low, high) = points.iter().fold((first, first), |(lo, hi), &p| (lo.min(p), hi.max(p)));
        // Use an edge direction in this end's own plane. Measuring only on the
        // whole beam's axes can give a diagonal line two nonzero spans.
        let along = points.iter().map(|&p| p - first)
            .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?
            .normalize_or_zero();
        let span = |axis| {
            let (lo, hi) = points.iter().map(|&p| (p - first).dot(axis))
                .fold((0.0f32, 0.0f32), |(lo, hi), p| (lo.min(p), hi.max(p)));
            hi - lo
        };
        Some((low, high, [span(along), span(along.perp())]))
    };
    let whole = section(f32::MIN, f32::MAX).ok_or("no vertices in section")?;
    let ends = [section(f32::MIN, 0.25).unwrap_or(whole), section(0.75, f32::MAX).unwrap_or(whole)];
    let spans = ends.map(|(_, _, spans)| spans);
    if spans.iter().flatten().any(|x| !x.is_finite() || *x <= 2e-3) {
        return Err("flat: no thickness across its axis");
    }
    // A beam fades from a bright window end (the ray models' cards do: alpha 0.10-0.12 near the
    // window, 0 at the far end); a glow that fades out both ways, or a patch of one alpha, is
    // not a beam [design: round 0's census, 2026-11-26].
    if !faded {
        return Err("no fade: the vertex alpha does not change");
    }
    if !directed {
        return Err("no bright end: the vertex alpha fades both ways");
    }
    let thin = |[u, v]: [f32; 2]| u.min(v) < LEAST_ASPECT * u.max(v);
    if spans.into_iter().any(thin) {
        return Err("a slab: the section is thinner than a tenth of its width");
    }
    if !length.is_finite() || length <= 0.0 {
        return Err("no length along its axis");
    }
    // The fitted prism still shares a centre at both ends.
    let [half_a, half_b] = ends.map(|(lo, hi, _)| [0, 1].map(|k| (lo[k] - mid[k]).abs().max((hi[k] - mid[k]).abs())));
    let a = square(mean) + u * mid[0] + v * mid[1] + axis * (start + lean[0] * mid[0] + lean[1] * mid[1]);
    if !a.is_finite() || !u.is_finite() || !v.is_finite() || !share.is_finite() {
        return Err("beam fit is not finite");
    }
    Ok(RayBeam { a, axis, length, u, v, half_a, half_b, lean, share })
}

/// Card centres within this share of the section's wide side of each other are one shaft
/// [design: round 0d, the window rays' cards, 2026-11-26].
pub const SHAFT_LINK: f32 = 0.12;
/// A card at least this share of the section's wide side across is an envelope (the long sheets
/// above and below a row of shafts), not a shaft of its own [design].
pub const ENVELOPE: f32 = 0.6;
/// A card runs along at least this share of the beam's length [design].
pub const CARD_ALONG: f32 = 0.5;
/// The most shafts a shape gives [design].
pub const MAX_SHAFTS: usize = 6;

/// The shafts of a ray shape whose whole beam is `beam`: its cards (the connected pieces of its
/// triangles) that run along the beam and are not envelopes, grouped by where their centres sit
/// across the section (single linkage within [`SHAFT_LINK`] of the wide side; while more than
/// [`MAX_SHAFTS`] groups are left, the two whose centres are nearest merge). Each group is a prism on the whole
/// beam's axis, window plane and length, its section the box of the group's vertices across.
/// Empty when fewer than two groups are found [design].
fn ray_shafts(verts: &[Vec3], tris: &[[u32; 3]], beam: &RayBeam) -> Vec<RayBeam> {
    let n = verts.len();
    if n < 4 || tris.is_empty() {
        return Vec::new();
    }
    // The cards: connected pieces of the triangles.
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut a: usize) -> usize {
        while parent[a] != a {
            parent[a] = parent[parent[a]];
            a = parent[a];
        }
        a
    }
    let valid = |t: &&[u32; 3]| t.iter().all(|&i| (i as usize) < n);
    for t in tris.iter().filter(valid) {
        let [a, b, c] = t.map(|i| i as usize);
        for (x, y) in [(a, b), (b, c)] {
            let (rx, ry) = (root(&mut parent, x), root(&mut parent, y));
            if rx != ry {
                parent[rx] = ry;
            }
        }
    }
    let mut pieces: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut seen = vec![false; n];
    for t in tris.iter().filter(valid) {
        for i in t.map(|i| i as usize) {
            if !seen[i] {
                seen[i] = true;
                let r = root(&mut parent, i);
                pieces.entry(r).or_default().push(i);
            }
        }
    }
    // Across the section, from the bright end's middle; along the axis.
    let across = |i: usize| [(verts[i] - beam.a).dot(beam.u), (verts[i] - beam.a).dot(beam.v)];
    let along = |i: usize| verts[i].dot(beam.axis);
    let range = |idx: &[usize], f: &dyn Fn(usize) -> f32| idx.iter().fold((f32::MAX, f32::MIN), |(l, h), &i| (l.min(f(i)), h.max(f(i))));
    let all: Vec<usize> = (0..n).filter(|&i| seen[i]).collect();
    let spread = [0, 1].map(|k| {
        let (l, h) = range(&all, &|i| across(i)[k]);
        h - l
    });
    let wide = spread[0].max(spread[1]);
    if wide <= 1e-3 {
        return Vec::new();
    }
    let mut keys: Vec<usize> = pieces.keys().copied().collect();
    keys.sort_unstable();
    let mut cards: Vec<([f32; 2], Vec<usize>)> = Vec::new();
    for key in keys {
        let idx = &pieces[&key];
        if idx.len() < 3 {
            continue;
        }
        let (l, h) = range(idx, &along);
        if h - l < CARD_ALONG * beam.length {
            continue;
        }
        let ext = [0, 1].map(|k| {
            let (l, h) = range(idx, &|i| across(i)[k]);
            h - l
        });
        if ext[0].max(ext[1]) >= ENVELOPE * wide {
            continue;
        }
        let centre = idx.iter().fold([0.0f32; 2], |c, &i| {
            let p = across(i);
            [c[0] + p[0], c[1] + p[1]]
        });
        cards.push((centre.map(|c| c / idx.len() as f32), idx.clone()));
    }
    if cards.len() < 2 {
        return Vec::new();
    }
    let link = SHAFT_LINK * wide;
    let m = cards.len();
    let mut up: Vec<usize> = (0..m).collect();
    for i in 0..m {
        for j in i + 1..m {
            let (a, b) = (cards[i].0, cards[j].0);
            if (a[0] - b[0]).hypot(a[1] - b[1]) <= link {
                let (ri, rj) = (root(&mut up, i), root(&mut up, j));
                if ri != rj {
                    up[ri] = rj;
                }
            }
        }
    }
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for i in 0..m {
        let r = root(&mut up, i);
        match groups.iter_mut().find(|g| g.0 == r) {
            Some(g) => g.1.push(i),
            None => groups.push((r, vec![i])),
        }
    }
    // Too many: the two groups whose centres (the mean of their cards') are nearest merge.
    let centre = |g: &[usize]| g.iter().fold([0.0f32; 2], |c, &i| [c[0] + cards[i].0[0] / g.len() as f32, c[1] + cards[i].0[1] / g.len() as f32]);
    while groups.len() > MAX_SHAFTS {
        let mut best = (f32::MAX, 0, 1);
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                let (a, b) = (centre(&groups[i].1), centre(&groups[j].1));
                let d = (a[0] - b[0]).hypot(a[1] - b[1]);
                if d < best.0 {
                    best = (d, i, j);
                }
            }
        }
        let gone = groups.remove(best.2);
        groups[best.1].1.extend(gone.1);
    }
    if groups.len() < 2 {
        return Vec::new();
    }
    groups
        .into_iter()
        .map(|(_, members)| {
            let idx: Vec<usize> = members.iter().flat_map(|&c| cards[c].1.iter().copied()).collect();
            let [(x0, x1), (y0, y1)] = [0, 1].map(|k| range(&idx, &|i| across(i)[k]));
            let (cx, cy) = (0.5 * (x0 + x1), 0.5 * (y0 + y1));
            let half = [(0.5 * (x1 - x0)).max(0.5), (0.5 * (y1 - y0)).max(0.5)];
            RayBeam {
                a: beam.a + beam.u * cx + beam.v * cy + beam.axis * (beam.lean[0] * cx + beam.lean[1] * cy),
                half_a: half,
                half_b: half,
                ..*beam
            }
        })
        .collect()
}

/// The plane t = c + s0 x + s1 y nearest the points (least squares); `None` when they do not
/// tell one, or it misses them by more than a tenth of their spread (10 units at least).
fn fit_plane(points: &[([f32; 2], f32)]) -> Option<(f32, [f32; 2])> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let m = points.iter().fold([0.0f64; 3], |m, (p, t)| [m[0] + p[0] as f64 / n, m[1] + p[1] as f64 / n, m[2] + *t as f64 / n]);
    let (mut xx, mut xy, mut yy, mut xt, mut yt) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, t) in points {
        let (x, y, t) = (p[0] as f64 - m[0], p[1] as f64 - m[1], *t as f64 - m[2]);
        xx += x * x;
        xy += x * y;
        yy += y * y;
        xt += x * t;
        yt += y * t;
    }
    let det = xx * yy - xy * xy;
    if det <= 1e-9 * (xx * yy).max(1e-9) {
        return None;
    }
    let s = [((xt * yy - yt * xy) / det) as f32, ((yt * xx - xt * xy) / det) as f32];
    let c = (m[2] - s[0] as f64 * m[0] - s[1] as f64 * m[1]) as f32;
    let miss = (points.iter().map(|(p, t)| (t - c - s[0] * p[0] - s[1] * p[1]).powi(2)).sum::<f32>() / points.len() as f32).sqrt();
    let spread = points.iter().map(|(p, _)| p[0].hypot(p[1])).fold(0.0f32, f32::max);
    (miss <= 10.0f32.max(0.1 * spread)).then_some((c, s))
}

/// The ray shapes of the model at `path` (an archive path), read from its file; empty when
/// it has none or cannot be read.
pub fn ray_shapes(data: &GameData, path: &str) -> Vec<RayShape> {
    crate::guard::guarded(|| Ok::<_, String>(read_rays(data, path))).ok().flatten().unwrap_or_default()
}

fn read_rays(data: &GameData, path: &str) -> Option<Vec<RayShape>> {
    let bytes = data.read(path)?;
    let schema = data.schema.clone();
    let file = NifFile::parse(&bytes, Some(&schema)).ok()?;
    let dec = Decoded::new(&file, &bytes, &schema);
    let objects = scene_objects(&file, &dec);
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let shaded = shaded_model(&file, &dec);
    let mut out = Vec::new();
    for o in objects.iter().filter(|o| !o.is_node && SHAPE_TYPES.contains(&o.type_name.as_str())) {
        let props = shape_props(&file, &dec, o, &by_block);
        if !props.unlit_part() || !props.external_emittance() || props.is_helper(shaded) || shape_alpha_mode(&props) != AlphaMode::Add {
            continue;
        }
        let Ok(shape) = &dec.blocks[o.block] else { continue };
        let Some(d) = shape.block_ref("data").and_then(|r| dec.get(r)) else { continue };
        // In the shape's own space, as its mesh (nif_spawn) is.
        let verts: Vec<Vec3> = d.floats("vertices").map(|f| f.as_chunks::<3>().0.iter().map(|v| Vec3::from_array(*v)).collect()).unwrap_or_default();
        let alphas = vertex_alphas(d.floats("vertex_colors"), props.vertex_alpha_part());
        let tris = triangle_indices(d);
        let beam = ray_beam_why(&verts, &tris, alphas.as_deref());
        let shafts = beam.as_ref().map(|b| ray_shafts(&verts, &tris, b)).unwrap_or_default();
        out.push(RayShape { block: o.block as u32, name: o.name.clone(), beam, shafts });
    }
    Some(out)
}

/// Fit the alpha actually used by the material, with the same cleanup as the drawn mesh.
fn vertex_alphas(colors: Option<&[f32]>, vertex_alpha: bool) -> Option<Vec<f32>> {
    let colors = colors.filter(|_| vertex_alpha)?;
    Some(colors.as_chunks::<4>().0.iter().map(|c| clean_vertex_color(*c, true)[3]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fan of flat cards swept along `sweep` from the plane y = 0: each card a chord of
    /// the section (x within 50, z within 240), with a dim row near the window.
    fn fan(sweep: Vec3) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<f32>) {
        fan_of(sweep, 50.0, [(0.0, 0.0), (0.03, 0.12), (1.0, 0.0)])
    }

    /// [`fan`] with its narrow half width and its rows (share of the sweep, alpha) given.
    fn fan_of(sweep: Vec3, narrow: f32, rows: [(f32, f32); 3]) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<f32>) {
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..12 {
            let turn = k as f32 / 12.0 * std::f32::consts::TAU;
            let p = Vec3::new(narrow * turn.cos(), 0.0, 240.0 * turn.sin());
            let q = Vec3::new(narrow * (turn + 2.4).cos(), 0.0, 240.0 * (turn + 2.4).sin());
            let base = verts.len() as u32;
            for (share, alpha) in rows {
                verts.extend([p + sweep * share, q + sweep * share]);
                alphas.extend([alpha, alpha]);
            }
            for row in 0..2 {
                let r = base + 2 * row;
                tris.extend([[r, r + 1, r + 2], [r + 1, r + 3, r + 2]]);
            }
        }
        (verts, tris, alphas)
    }

    #[test]
    fn a_fan_of_cards_gives_its_sweep_and_its_leaning_window() {
        let sweep = Vec3::new(0.0, -716.0, -325.0);
        let (verts, tris, alphas) = fan(sweep);
        let b = ray_beam(&verts, &tris, Some(&alphas)).expect("a beam");
        assert!(b.axis.dot(sweep.normalize()) > 0.9999, "{b:?}");
        assert!((b.length - sweep.length()).abs() < 1.0, "{b:?}");
        assert!(b.is_long());
        // The section: 240 tall, seen leaned by the sweep, and 50 wide; the same at both ends.
        let tall = 240.0 * sweep.normalize().y.abs();
        assert!((b.half_a[0] - tall).abs() < 4.0 && (b.half_a[1] - 50.0).abs() < 4.0, "{b:?}");
        assert!((b.half_a[0] - b.half_b[0]).abs() < 2.0 && (b.half_a[1] - b.half_b[1]).abs() < 2.0, "{b:?}");
        // Its bright end is the window's plane y = 0, middle and corners.
        for (x, y) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.5)] {
            let (x, y) = (x * b.half_a[0], y * b.half_a[1]);
            let p = b.a + b.u * x + b.v * y + b.axis * (b.lean[0] * x + b.lean[1] * y);
            assert!(p.y.abs() < 5.0, "{p:?}");
        }
        assert!(b.a.length() < 5.0, "{b:?}");
        // A right-handed frame.
        assert!((b.u.cross(b.v) - b.axis).length() < 1e-4);
        // Wound the other way: the same bright end.
        let back: Vec<[u32; 3]> = tris.iter().map(|t| [t[2], t[1], t[0]]).collect();
        assert!(ray_beam(&verts, &back, Some(&alphas)).expect("a beam").axis.dot(b.axis) > 0.9999);
    }

    #[test]
    fn a_fade_requires_vertex_alpha_in_the_material() {
        let (verts, tris, alphas) = fan(Vec3::new(0.0, -716.0, -325.0));
        let colors: Vec<_> = alphas.iter().flat_map(|&alpha| [1.0, 1.0, 1.0, alpha]).collect();
        let drawn = vertex_alphas(Some(&colors), true);
        let ignored = vertex_alphas(Some(&colors), false);
        assert!(ray_beam(&verts, &tris, drawn.as_deref()).is_some());
        assert!(ray_beam(&verts, &tris, ignored.as_deref()).is_none());
        let nonfinite = [1.0, 1.0, 1.0, f32::NAN, 1.0, 1.0, 1.0, -1.0];
        assert_eq!(vertex_alphas(Some(&nonfinite), true), Some(vec![1.0, 0.0]));
    }

    /// Four crossed cards, each with a spine, 4 wide at the bright end (z = 0)
    /// and 60 wide at z = -100. The section keeps its aspect across the taper.
    fn crossed_cone() -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<f32>) {
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..4 {
            let turn = k as f32 * std::f32::consts::FRAC_PI_4;
            let side = Vec3::new(turn.cos(), turn.sin(), 0.0);
            let base = verts.len() as u32;
            for (z, w, alpha) in [(0.0, 2.0, 1.0), (-100.0, 30.0, 0.0)] {
                for x in [-1.0, 0.0, 1.0] {
                    verts.push(side * (x * w) + Vec3::Z * z);
                    alphas.push(alpha);
                }
            }
            tris.extend([[0, 1, 3], [1, 4, 3], [1, 2, 4], [2, 5, 4]].map(|t: [u32; 3]| t.map(|i| i + base)));
        }
        (verts, tris, alphas)
    }

    #[test]
    fn a_cone_widens_toward_its_far_end() {
        let (verts, tris, alphas) = crossed_cone();
        let b = ray_beam(&verts, &tris, Some(&alphas)).expect("a beam");
        assert!(b.axis.z < -0.99, "{b:?}");
        assert!(b.half_b[0] > 10.0 * b.half_a[0] && b.half_b[1] > 10.0 * b.half_a[1], "{b:?}");
        assert!((b.length - 100.0).abs() < 1.0 && b.lean == [0.0; 2], "{b:?}");
    }

    #[test]
    fn a_flat_end_offset_from_the_whole_section_is_rejected() {
        let (mut verts, tris, alphas) = crossed_cone();
        for (i, p) in verts.iter_mut().enumerate() {
            // Collapse the bright end onto one line. The spines still run along z,
            // with their x offset from the wide end's centre.
            if p.z == 0.0 || i % 6 == 4 {
                p.x = 4.0;
            }
        }
        assert_eq!(ray_beam_why(&verts, &tris, Some(&alphas)), Err("flat: no thickness across its axis"));
    }

    /// Cards for the shaft tests, made for them: each a strip along -z from the window plane
    /// z = 0 to z = -800 (alpha 0 at the window, 0.12 at z = -24, 0 at the far end), between two
    /// points across.
    fn card(verts: &mut Vec<Vec3>, tris: &mut Vec<[u32; 3]>, alphas: &mut Vec<f32>, p: Vec3, q: Vec3) {
        let base = verts.len() as u32;
        for (z, alpha) in [(0.0, 0.0), (-24.0, 0.12), (-800.0, 0.0)] {
            verts.extend([p + Vec3::Z * z, q + Vec3::Z * z]);
            alphas.extend([alpha, alpha]);
        }
        for row in 0..2 {
            let r = base + 2 * row;
            tris.extend([[r, r + 1, r + 2], [r + 1, r + 3, r + 2]]);
        }
    }

    /// Three bundles of two crossed cards (x within 20 of -200, 0 and 200 at y = 0; y within 40
    /// at their x), under two envelope sheets across the whole width (x within 240 at y = +-40):
    /// three shafts, each 40 by 80 on the whole beam's axis, window and length.
    #[test]
    fn three_bundles_of_cards_give_three_shafts() {
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        for x in [-200.0f32, 0.0, 200.0] {
            card(&mut verts, &mut tris, &mut alphas, Vec3::new(x - 20.0, 0.0, 0.0), Vec3::new(x + 20.0, 0.0, 0.0));
            card(&mut verts, &mut tris, &mut alphas, Vec3::new(x, -40.0, 0.0), Vec3::new(x, 40.0, 0.0));
        }
        for y in [-40.0f32, 40.0] {
            card(&mut verts, &mut tris, &mut alphas, Vec3::new(-240.0, y, 0.0), Vec3::new(240.0, y, 0.0));
        }
        let whole = ray_beam_why(&verts, &tris, Some(&alphas)).expect("a beam");
        assert!(whole.axis.z < -0.9999 && (whole.length - 800.0).abs() < 1.0, "{whole:?}");
        let shafts = ray_shafts(&verts, &tris, &whole);
        assert_eq!(shafts.len(), 3, "{shafts:?}");
        let mut xs: Vec<f32> = shafts.iter().map(|s| s.a.x).collect();
        xs.sort_by(f32::total_cmp);
        for (x, want) in xs.iter().zip([-200.0, 0.0, 200.0]) {
            assert!((x - want).abs() < 0.5, "{xs:?}");
        }
        for s in &shafts {
            assert!(s.a.y.abs() < 0.5 && s.a.z.abs() < 0.5, "{s:?}");
            assert!(s.axis.dot(whole.axis) > 0.9999 && s.length == whole.length && s.lean == whole.lean);
            // The section: 40 along x (half 20) and 80 along y (half 40), in the whole beam's u, v.
            let hx = if s.u.x.abs() > 0.9 { s.half_a[0] } else { s.half_a[1] };
            let hy = if s.u.x.abs() > 0.9 { s.half_a[1] } else { s.half_a[0] };
            assert!((hx - 20.0).abs() < 0.5 && (hy - 40.0).abs() < 0.5, "{s:?}");
            assert_eq!(s.half_a, s.half_b);
        }
    }

    /// Cards spread evenly across the width (centres 30 apart, under the link): no shafts, the
    /// whole beam stands alone; the PROTO-style fan of one window too.
    #[test]
    fn evenly_spread_cards_give_no_shafts() {
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..14 {
            let x = -195.0 + 30.0 * k as f32;
            card(&mut verts, &mut tris, &mut alphas, Vec3::new(x, -40.0, 0.0), Vec3::new(x, 40.0, 0.0));
        }
        let whole = ray_beam_why(&verts, &tris, Some(&alphas)).expect("a beam");
        assert!(ray_shafts(&verts, &tris, &whole).is_empty());
        // One bundle alone is one group: no shafts either.
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        card(&mut verts, &mut tris, &mut alphas, Vec3::new(-20.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0));
        card(&mut verts, &mut tris, &mut alphas, Vec3::new(0.0, -40.0, 0.0), Vec3::new(0.0, 40.0, 0.0));
        let whole = ray_beam_why(&verts, &tris, Some(&alphas)).expect("a beam");
        assert!(ray_shafts(&verts, &tris, &whole).is_empty());
    }

    /// Eight cards 100 apart across 700 (the link is 84): eight groups, the nearest merged down
    /// to [`MAX_SHAFTS`].
    #[test]
    fn shafts_are_capped() {
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..8 {
            let x = -350.0 + 100.0 * k as f32;
            card(&mut verts, &mut tris, &mut alphas, Vec3::new(x, -60.0, 0.0), Vec3::new(x, 60.0, 0.0));
        }
        let whole = ray_beam_why(&verts, &tris, Some(&alphas)).expect("a beam");
        assert_eq!(ray_shafts(&verts, &tris, &whole).len(), MAX_SHAFTS);
    }

    /// The census's misfits (round 0, LakeMeadCaveINT), made for the test: a patch of three stacked
    /// squares of one alpha (as the floor spot is), a fan whose alpha peaks in its middle and
    /// fades both ways (as the half-round glow fills do), and a fan too thin across.
    #[test]
    fn a_patch_a_glow_and_a_slab_are_no_beams() {
        let (mut verts, mut tris) = (Vec::new(), Vec::new());
        for z in [-2.4f32, 0.0, 2.4] {
            let base = verts.len() as u32;
            verts.extend([Vec3::new(-64.0, -64.0, z), Vec3::new(64.0, -64.0, z), Vec3::new(64.0, 64.0, z), Vec3::new(-64.0, 64.0, z)]);
            tris.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
        }
        let alphas = vec![0.75; verts.len()];
        assert_eq!(ray_beam_why(&verts, &tris, Some(&alphas)), Err("no fade: the vertex alpha does not change"));
        let sweep = Vec3::new(0.0, -716.0, -325.0);
        let (verts, tris, alphas) = fan_of(sweep, 50.0, [(0.0, 0.0), (0.5, 0.4), (1.0, 0.0)]);
        assert_eq!(ray_beam_why(&verts, &tris, Some(&alphas)), Err("no bright end: the vertex alpha fades both ways"));
        let (verts, tris, alphas) = fan_of(sweep, 5.0, [(0.0, 0.0), (0.03, 0.12), (1.0, 0.0)]);
        assert_eq!(ray_beam_why(&verts, &tris, Some(&alphas)), Err("a slab: the section is thinner than a tenth of its width"));
        // The usual fan still is one.
        let (verts, tris, alphas) = fan(sweep);
        assert!(ray_beam_why(&verts, &tris, Some(&alphas)).is_ok());
    }

    /// A flat strip on the floor (as a light patch is): no beam, and the census's reason says why.
    #[test]
    fn a_flat_strip_is_no_beam_and_says_why() {
        let (mut verts, mut tris, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
        for (row, (y, alpha)) in [(0.0f32, 0.0f32), (-30.0, 0.12), (-400.0, 0.06), (-800.0, 0.0)].into_iter().enumerate() {
            for x in [-50.0f32, 0.0, 50.0] {
                verts.push(Vec3::new(x, y, 0.0));
                alphas.push(alpha);
            }
            if row > 0 {
                let (a, b) = (3 * (row as u32 - 1), 3 * row as u32);
                tris.extend([[a, a + 1, b], [a + 1, b + 1, b], [a + 1, a + 2, b + 1], [a + 2, b + 2, b + 1]]);
            }
        }
        assert_eq!(ray_beam_why(&verts, &tris, Some(&alphas)), Err("flat: no thickness across its axis"));
        assert_eq!(ray_beam_why(&[], &[], None), Err("fewer than 4 vertices"));
    }

    #[test]
    fn a_squat_fan_is_not_long_and_nothing_is_made_of_nothing() {
        let (verts, tris, alphas) = fan(Vec3::new(0.0, -150.0, -60.0));
        let b = ray_beam(&verts, &tris, Some(&alphas)).expect("a beam");
        assert!(!b.is_long(), "{b:?}");
        assert_eq!(ray_beam(&[], &[], None), None);
        assert_eq!(ray_beam(&[Vec3::ZERO; 8], &[[0, 1, 2], [3, 4, 5]], None), None);
        assert_eq!(ray_beam(&[Vec3::NAN; 8], &[[0, 1, 2]], None), None);
    }
}
