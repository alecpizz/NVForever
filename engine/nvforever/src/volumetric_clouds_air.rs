//! Camera-to-cloud air colour sampled from the weather's diffuse dome.
use super::{CloudVolumeUniform, VolumeInputs, MAX_LIGHT};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;

// The two dome rings bracketing a given elevation, and their interpolation weight.
#[derive(Clone, Copy)]
struct RingSample {
    below: usize,
    above: usize,
    weight: f32,
}

impl RingSample {
    fn at(positions: &[[f32; 3]], elevation: f32) -> Option<Self> {
        let mut below = None;
        let mut above = None;
        for (index, &p) in positions.iter().enumerate() {
            // Dome vertices use game coordinates, with Z up.
            let Some(dir) = Vec3::from_array(p).try_normalize() else { continue };
            if dir.z <= elevation && below.is_none_or(|(z, _)| dir.z > z) {
                below = Some((dir.z, index));
            }
            if dir.z >= elevation && above.is_none_or(|(z, _)| dir.z < z) {
                above = Some((dir.z, index));
            }
        }
        let (lo, i) = below.or(above)?;
        let (hi, j) = above.or(below)?;
        let weight = if hi > lo { (elevation - lo) / (hi - lo) } else { 0.0 };
        Some(Self { below: i, above: j, weight })
    }

    fn colour(self, colors: &[[f32; 4]], scale: f32) -> Option<Vec3> {
        let a = Vec4::from_array(*colors.get(self.below)?).truncate();
        let b = Vec4::from_array(*colors.get(self.above)?).truncate();
        let c = a.lerp(b, self.weight) * scale;
        c.is_finite().then(|| c.clamp(Vec3::ZERO, Vec3::splat(MAX_LIGHT)))
    }
}

/// Sample the actual diffuse vertex colours at the horizon and 20 degrees. The positions
/// are inspected once per dome. Its coloured vertices already track the hour and weather.
pub(super) struct Dome([RingSample; 2]);

impl Dome {
    pub(super) fn new(mesh: &Mesh) -> Option<Self> {
        let VertexAttributeValues::Float32x3(pos) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else { return None };
        Some(Self([RingSample::at(pos, 0.0)?, RingSample::at(pos, 20.0_f32.to_radians().sin())?]))
    }

    pub(super) fn write(&self, mesh: &Mesh, i: &VolumeInputs, u: &mut CloudVolumeUniform, strength: f32) {
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { return };
        let (Some(low), Some(high)) = (self.0[0].colour(colors, i.sky_scale), self.0[1].colour(colors, i.sky_scale)) else { return };
        u.air_low = low.extend(strength);
        u.air_high = high.extend(i.plane_height - i.above_ground);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dome_sample_uses_live_linear_colours_between_the_bracketing_rings() {
        let pos = [[1.0, 0.0, 0.0], [3.0, 0.0, 4.0]];
        let s = RingSample::at(&pos, 0.4).unwrap();
        let colors = [[0.1, 0.2, 0.3, 1.0], [0.3, 0.4, 0.5, 1.0]];
        assert!(s.colour(&colors, 2.0).unwrap().abs_diff_eq(Vec3::new(0.4, 0.6, 0.8), 1e-6));
        assert!(s.colour(&[], 1.0).is_none());
        assert!(RingSample::at(&[[0.0; 3]], 0.0).is_none());
    }
}
