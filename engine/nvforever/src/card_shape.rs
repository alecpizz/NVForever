//! A playing card's outline: a rectangle with rounded corners, for the card tables that
//! draw their cards themselves (Caravan, poker). docs/spec-notes/caravan.md §2.1.
//!
//! - The card pictures are opaque at their corners: the decks' faces and backs under
//!   `textures\terminals\nv_playingcards\` are DXT1 with no transparent texel, their
//!   corners a pale paper colour [observed: cardb_s09.dds, cardg_back.dds and
//!   cardt_c01.dds, 128 by 256, the first level's corner blocks].
//! - The original's own card models round the corners with their shape instead: the face
//!   of `meshes\terminals\nv_caravan\player_1_01.nif` is a 12-point outline 2.5 across and
//!   3.5 long, each corner cut by three points, (1.107, 1.75), (1.205, 1.705) and
//!   (1.25, 1.597) in its quarter [observed: its 12-vertex NiTriStripsData; the 24-vertex
//!   ones are the shadow round it]. Its UVs lay the whole picture, 0 to 1, on the full
//!   rectangle, each outline point at its own place (0.0572 and 0.0437 at a corner's
//!   ends) [observed]. Blackjack draws the game's card models, so it has them.
//! - NVForever draws a card as that outline fitted to the card's size (the corner points as
//!   fractions of the half width and half length), the picture laid on it as on the whole
//!   rectangle, so the picture's square corners outside the outline are not drawn [design].

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;

/// The top right corner's three points from the right edge round to the top edge, each as
/// (across, along): fractions of the half width and the half length [observed: the face of
/// player_1_01.nif, half width 1.25 and half length 1.75; the points (1.25, 1.597),
/// (1.205, 1.705), (1.107, 1.75)].
pub const CORNER: [Vec2; 3] = [Vec2::new(1.0, 1.597 / 1.75), Vec2::new(1.205 / 1.25, 1.705 / 1.75), Vec2::new(1.107 / 1.25, 1.0)];

/// The outline of a card `size` across (x) and long (y), about its middle, against the
/// clock seen from +Z: 12 points, three to a corner.
pub fn outline(size: Vec2) -> Vec<Vec2> {
    let half = size / 2.0;
    let mut out = Vec::with_capacity(12);
    // Top right, top left, bottom left, bottom right. Mirroring one axis turns the points'
    // order round, so those quarters take them backwards.
    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let mut pts: Vec<Vec2> = CORNER.iter().map(|c| Vec2::new(c.x * sx, c.y * sy) * half).collect();
        if sx * sy < 0.0 {
            pts.reverse();
        }
        out.extend(pts);
    }
    out
}

/// The picture coordinates at a point of a card `size` about its middle: the picture's top
/// left at the card's top left (+Y up), as on a whole rectangle.
fn uv_at(p: Vec2, size: Vec2) -> Vec2 {
    Vec2::new(p.x / size.x + 0.5, 0.5 - p.y / size.y)
}

/// A card's face (`up`: facing +Z) or back (facing -Z, its picture mirrored so it reads
/// from below), at height `z`: a fan of triangles from the middle to the outline.
pub fn card_mesh(size: Vec2, z: f32, up: bool) -> Mesh {
    let rim = outline(size);
    let normal = if up { Vec3::Z } else { -Vec3::Z };
    let mut pos = vec![[0.0, 0.0, z]];
    let mut uv = vec![[0.5, 0.5]];
    for p in &rim {
        pos.push(p.extend(z).to_array());
        let t = uv_at(*p, size);
        uv.push(if up { t.to_array() } else { [1.0 - t.x, t.y] });
    }
    let n = rim.len() as u32;
    let mut idx = Vec::with_capacity(3 * n as usize);
    for i in 0..n {
        let (a, b) = (1 + i, 1 + (i + 1) % n);
        idx.extend(if up { [0, a, b] } else { [0, b, a] });
    }
    let count = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal.to_array(); count]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// Whether a picture coordinate is drawn on a card mesh: inside one of its triangles.
    fn covers(mesh: &Mesh, at: Vec2) -> bool {
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { return false };
        let Some(Indices::U32(idx)) = mesh.indices() else { return false };
        idx.chunks(3).any(|t| {
            let [a, b, c] = [uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]].map(Vec2::from);
            let s = |p: Vec2, q: Vec2| (q - p).perp_dot(at - p);
            let d = [s(a, b), s(b, c), s(c, a)];
            !(d.iter().any(|&x| x < -1e-6) && d.iter().any(|&x| x > 1e-6))
        })
    }

    /// The card picture's square corners are not drawn; its middle, its edges' middles and
    /// the picture just inside a rounded corner are [observed: the pictures are opaque at
    /// their corners, so the shape is what hides them].
    #[test]
    fn a_cards_picture_corners_are_cut_away() {
        for size in [Vec2::new(111.0, 147.0), Vec2::new(6.3, 8.8), Vec2::ONE] {
            for up in [true, false] {
                let m = card_mesh(size, 0.0, up);
                for corner in [Vec2::new(0.004, 0.004), Vec2::new(0.996, 0.004), Vec2::new(0.004, 0.996), Vec2::new(0.996, 0.996)] {
                    assert!(!covers(&m, corner), "{size} up {up}: the picture's corner {corner} is drawn");
                }
                for inside in [Vec2::splat(0.5), Vec2::new(0.5, 0.002), Vec2::new(0.002, 0.5), Vec2::new(0.07, 0.07), Vec2::new(0.93, 0.93)] {
                    assert!(covers(&m, inside), "{size} up {up}: {inside} is not drawn");
                }
            }
        }
    }

    /// The outline is the original's: 12 points, each corner at the measured places, and
    /// convex against the clock.
    #[test]
    fn the_outline_is_the_original_cards() {
        let o = outline(Vec2::new(2.5, 3.5));
        assert_eq!(o.len(), 12);
        for want in [Vec2::new(1.107, 1.75), Vec2::new(1.205, 1.705), Vec2::new(1.25, 1.597)] {
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                let w = Vec2::new(want.x * sx, want.y * sy);
                assert!(o.iter().any(|p| p.distance(w) < 1e-4), "{w} not on the outline {o:?}");
            }
        }
        for i in 0..12 {
            let (a, b, c) = (o[i], o[(i + 1) % 12], o[(i + 2) % 12]);
            assert!((b - a).perp_dot(c - b) >= -1e-6, "the outline turns the wrong way at {b}");
        }
    }
}
