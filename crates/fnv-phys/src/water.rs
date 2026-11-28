//! Water volumes: where the character swims (physics.md §6.6).
//!
//! Two kinds. Cell water: per loaded exterior cell, everything below a flat surface over
//! the grid square of the cell (`Cell::water_surface` of `fnv-world`: CELL XCLW, else the
//! worldspace's default height, for cells flagged "has water"); interiors have none (an
//! interior's XCLW was refuted as a water surface, §6.6, and [`WaterArea::Interior`] is
//! only kept for callers that set one explicitly). Placed water: a reference whose model
//! holds phantom shapes on the Water layer (the placeable water models, PWAT), a box in
//! the reference's frame whose top is the surface [hypothesis: one interior,
//! `swim_h2o_drops`]. Volumes have no collider: the character asks for the surface above
//! its feet.

use crate::world::{PhysWorld, RefPose};
use fnv_collide::CollisionObject;
use fnv_world::{cell_of, CELL_SIZE};
use std::collections::BTreeMap;

/// Where a water surface applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaterArea {
    /// The exterior grid square (x, y), `CELL_SIZE` units wide.
    Exterior(i32, i32),
    /// The whole of the interior being simulated.
    Interior,
}

impl WaterArea {
    /// Whether the point (x, y) lies in this area.
    pub fn contains(self, x: f32, y: f32) -> bool {
        match self {
            WaterArea::Exterior(gx, gy) => cell_of(x, y) == (gx, gy),
            WaterArea::Interior => true,
        }
    }

    /// South-west corner and size of an exterior square, for callers that draw it.
    pub fn bounds(self) -> Option<([f32; 2], f32)> {
        match self {
            WaterArea::Exterior(gx, gy) => Some(([gx as f32 * CELL_SIZE, gy as f32 * CELL_SIZE], CELL_SIZE)),
            WaterArea::Interior => None,
        }
    }
}

/// One water volume of a placed reference: a box in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedWater {
    /// The placed reference.
    pub form_id: u32,
    /// Box centre, world coordinates.
    pub centre: [f32; 3],
    /// Box axes: the reference's rotation (row-major 3×3; column `i` is local axis `i`).
    pub rotation: [f32; 9],
    /// Half extents along the box axes, game units.
    pub half: [f32; 3],
    /// The surface: the highest point of the box.
    pub surface: f32,
}

impl PlacedWater {
    /// The volumes of a reference's water objects ([`crate::RefCollision::water`]): each
    /// object's bounds in the model (`CollisionObject::root_bounds`), scaled, rotated and
    /// moved by the reference [hypothesis: the phantom's box is the water, its top the
    /// surface; in every placeable water model the box lies under the visible surface
    /// quad with its top at the quad, physics.md §6.6].
    pub fn from_ref(pose: &RefPose, objects: &[CollisionObject]) -> Vec<PlacedWater> {
        let r = pose.rotation;
        let rot = |v: [f32; 3]| {
            [r[0] * v[0] + r[1] * v[1] + r[2] * v[2], r[3] * v[0] + r[4] * v[1] + r[5] * v[2], r[6] * v[0] + r[7] * v[1] + r[8] * v[2]]
        };
        objects
            .iter()
            .filter_map(|o| {
                let b = o.root_bounds();
                if !b.is_valid() {
                    return None;
                }
                let c = rot(b.center().map(|v| v * pose.scale));
                let centre = [pose.position[0] + c[0], pose.position[1] + c[1], pose.position[2] + c[2]];
                let half = b.extent().map(|v| v * pose.scale / 2.0);
                let reach = (0..3).map(|i| (r[6 + i] * half[i]).abs()).sum::<f32>();
                Some(PlacedWater { form_id: pose.form_id, centre, rotation: r, half, surface: centre[2] + reach })
            })
            .collect()
    }

    /// The point in the box's own axes, relative to its centre.
    fn local(&self, p: [f32; 3]) -> [f32; 3] {
        let d = [p[0] - self.centre[0], p[1] - self.centre[1], p[2] - self.centre[2]];
        let r = self.rotation;
        [0, 1, 2].map(|i| r[i] * d[0] + r[3 + i] * d[1] + r[6 + i] * d[2])
    }

    /// Whether the point (x, y) lies in the box's footprint (any height): for an upright
    /// box, within its x/y extent.
    pub fn covers(&self, x: f32, y: f32) -> bool {
        let l = self.local([x, y, self.centre[2]]);
        l[0].abs() <= self.half[0] && l[1].abs() <= self.half[1]
    }

    /// Whether the point lies over or in the box: in its footprint and not below its
    /// bottom.
    pub fn holds_below(&self, p: [f32; 3]) -> bool {
        let l = self.local(p);
        l[0].abs() <= self.half[0] && l[1].abs() <= self.half[1] && l[2] >= -self.half[2]
    }
}

/// The water of a physics world: cell surfaces and placed volumes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Waters {
    cells: BTreeMap<WaterArea, f32>,
    placed: BTreeMap<u32, Vec<PlacedWater>>,
}

impl Waters {
    pub(crate) fn remove_ref(&mut self, form_id: u32) -> bool {
        self.placed.remove(&form_id).is_some()
    }
}

impl PhysWorld {
    /// Set (or with `None` remove) the water surface height of an area.
    pub fn set_water(&mut self, area: WaterArea, surface: Option<f32>) {
        match surface.filter(|s| s.is_finite()) {
            Some(s) => {
                self.water.cells.insert(area, s);
            }
            None => {
                self.water.cells.remove(&area);
            }
        }
    }

    /// Add the water volumes of a placed reference (replacing any it had); they go with
    /// the reference ([`PhysWorld::remove_ref`]). Returns how many volumes it has.
    pub fn add_placed_water(&mut self, pose: &RefPose, objects: &[CollisionObject]) -> usize {
        let v = PlacedWater::from_ref(pose, objects);
        let n = v.len();
        if n == 0 {
            self.water.placed.remove(&pose.form_id);
        } else {
            self.water.placed.insert(pose.form_id, v);
        }
        n
    }

    /// The water surface height above or below the point (x, y), if it lies in water: an
    /// interior's water, else the highest placed volume whose footprint holds it, else the
    /// exterior square containing it.
    pub fn water_surface_at(&self, x: f32, y: f32) -> Option<f32> {
        if let Some(&s) = self.water.cells.get(&WaterArea::Interior) {
            return Some(s);
        }
        let placed = self.water.placed.values().flatten().filter(|w| w.covers(x, y)).map(|w| w.surface).reduce(f32::max);
        if placed.is_some() {
            return placed;
        }
        let (gx, gy) = cell_of(x, y);
        self.water.cells.get(&WaterArea::Exterior(gx, gy)).copied()
    }

    /// The surface of the water a point is in or over: as [`PhysWorld::water_surface_at`],
    /// except that a placed volume counts only down to its bottom.
    pub fn water_surface_for(&self, p: [f32; 3]) -> Option<f32> {
        self.water_surface_with_receiver(p).map(|(surface, _)| surface)
    }

    /// Like `water_surface_for`, with the reference ID of the selected placed water.
    /// Zero identifies continuous cell water.
    pub fn water_surface_with_receiver(&self, p: [f32; 3]) -> Option<(f32, u32)> {
        if let Some(&surface) = self.water.cells.get(&WaterArea::Interior) {
            return Some((surface, 0));
        }
        let placed = self.water.placed.values().flatten()
            .filter(|water| water.holds_below(p))
            .reduce(|a, b| if b.surface > a.surface { b } else { a });
        if let Some(water) = placed {
            return Some((water.surface, water.form_id));
        }
        let (gx, gy) = cell_of(p[0], p[1]);
        self.water.cells.get(&WaterArea::Exterior(gx, gy)).map(|&surface| (surface, 0))
    }

    /// The water areas with their surface heights, in order.
    pub fn water(&self) -> impl Iterator<Item = (WaterArea, f32)> + '_ {
        self.water.cells.iter().map(|(&a, &s)| (a, s))
    }

    /// The placed water volumes, in reference order.
    pub fn placed_water(&self) -> impl Iterator<Item = &PlacedWater> + '_ {
        self.water.placed.values().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PhysParams;

    #[test]
    fn the_surface_and_receiver_share_physics_precedence() {
        let mut world = PhysWorld::new(PhysParams::default());
        world.set_water(WaterArea::Exterior(0, 0), Some(10.0));
        let pool = PlacedWater {
            form_id: 42,
            centre: [10.0, 10.0, 8.0],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            half: [5.0, 5.0, 2.0],
            surface: 10.0,
        };
        world.water.placed.insert(42, vec![pool]);
        assert_eq!(world.water_surface_with_receiver([10.0, 10.0, 10.0]), Some((10.0, 42)));
        assert_eq!(world.water_surface_with_receiver([10.0, 10.0, 5.0]), Some((10.0, 0)));

        let upper = PlacedWater { form_id: 43, centre: [10.0, 10.0, 18.0], surface: 20.0, ..pool };
        world.water.placed.insert(43, vec![upper]);
        assert_eq!(world.water_surface_with_receiver([10.0, 10.0, 19.0]), Some((20.0, 43)));
        assert_eq!(world.water_surface_with_receiver([10.0, 10.0, 10.0]), Some((10.0, 42)));

        world.set_water(WaterArea::Interior, Some(10.0));
        assert_eq!(world.water_surface_with_receiver([10.0, 10.0, 10.0]), Some((10.0, 0)));
    }

    #[test]
    fn surfaces_follow_their_squares() {
        let mut w = PhysWorld::new(PhysParams::default());
        assert_eq!(w.water_surface_at(10.0, 10.0), None);
        w.set_water(WaterArea::Exterior(13, 9), Some(2600.0));
        assert_eq!(w.water_surface_at(13.0 * 4096.0 + 1.0, 9.0 * 4096.0 + 1.0), Some(2600.0));
        assert_eq!(w.water_surface_at(14.0 * 4096.0 + 1.0, 9.0 * 4096.0 + 1.0), None);
        assert_eq!(w.water_surface_at(13.0 * 4096.0 - 1.0, 9.0 * 4096.0 + 1.0), None);
        w.set_water(WaterArea::Exterior(13, 9), None);
        assert_eq!(w.water_surface_at(13.0 * 4096.0 + 1.0, 9.0 * 4096.0 + 1.0), None);
        w.set_water(WaterArea::Interior, Some(204.0));
        assert_eq!(w.water_surface_at(-1.0e5, 3.0), Some(204.0));
        assert_eq!(w.water().count(), 1);
    }
}
