//! Under-water fog: while the camera is below the water surface of its cell, the
//! camera's distance fog becomes the water type's under-water fog (WATR DNAM under fog
//! near / far [documented: xEdit names]). Its colour goes from the water type's shallow
//! colour at the surface to its deep colour [`DEEP_AT`] units down, and is not lit: the
//! picture's grade (the image space by time of day) does the rest ([`fog_bytes`];
//! world.md §6 "Under-water colour", #2916) [observed in the original, hypothesis for the rule]. Leaving the
//! water hands the fog back to the weather.
//! Water effects also increases density with camera depth, capped at 2.5x, using
//! each type's above-water deep-plane reference [hypothesis; an optional enhancement].

use crate::sky::SkyState;
use crate::water::DistantWater;
use crate::world::{Streamer, WorldData, bevy_to_game};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use fnv_sim::FormId;
use fnv_world::cell_of;
use fnv_world::sky::{Water, srgb_to_linear};
use nvf_play::phys::PhysState;
use nvf_play::sim::SimState;
use std::collections::HashMap;

/// Whether the main camera is under water this frame (set by [`underwater_fog`], so in
/// `PostUpdate`, where the app has the resource: the volumetric clouds are not drawn while it is).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CameraSubmerged(pub bool);

/// Apply the water override to the same camera transform the renderer extracts.
pub struct UnderwaterFogPlugin;

impl Plugin for UnderwaterFogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, underwater_fog.after(bevy::transform::TransformSystems::Propagate));
    }
}

/// Water types read so far, by FormID.
#[derive(Default)]
pub struct WaterTypes(HashMap<u32, Option<Water>>);

/// The water surface the camera is under, with its type: outdoors the camera cell's
/// water when the camera is below it; indoors the physics' water the camera is in (placed
/// water volumes), coloured by that volume's own PWAT water type ([`fog_water_type`]).
/// An interior's own XCLW is not water [observed: physics.md §6.6, `swim_h2o_drops`,
/// `sw_guardian`]: e.g. The Tops' 13th floor and presidential suite store XCLW 14,095.9,
/// 5,000+ units above every reference in them, and reading it as a surface drew the
/// under-water fog (WastelandNV's Potomac, −1,000..700 units) round the camera there
/// (world.md §8 "Interior XCLW is not water"). Without physics (the viewer, no `--play`)
/// an interior has no under-water fog.
#[allow(clippy::too_many_arguments)]
fn water_above(
    world: &WorldData,
    streamer: &Streamer,
    dw: &mut DistantWater,
    phys: Option<&PhysState>,
    sim: Option<&SimState>,
    placed_types: &mut HashMap<u32, Option<u32>>,
    cam_game: Vec3,
    enhanced: bool,
) -> Option<(f32, u32)> {
    if enhanced && streamer.interior.is_none() {
        let ws = world.index.worldspaces.get(&world.world)?;
        let c = world.grid_cell(cell_of(cam_game.x, cam_game.y));
        // Match the near-cell plane, including No LOD Water cells. No LAND decode:
        // the opaque depth hides the part of this plane below the terrain.
        let cell_water = c.and_then(|c| Some((c.water_surface(ws.default_water_height)?, c.water_type.or(ws.water)?)));
        // Exterior placed volumes already known to physics can override the cell's
        // plane, just as indoors. Do not infer a volume from interior XCLW.
        let placed_water = phys.and_then(|phys| {
            let p = cam_game.to_array();
            let placed = phys.world.placed_water().filter(|w| w.holds_below(p)).max_by(|a, b| a.surface.total_cmp(&b.surface))?;
            let wid = *placed_types.entry(placed.form_id).or_insert_with(|| {
                let base = sim?.refs.get(FormId(placed.form_id))?.base;
                fnv_world::sky::placeable_water_type(world.lo, world.schema, base.0)
            });
            Some((placed.surface, fog_water_type(wid, c.and_then(|c| c.water_type), ws.water)?))
        });
        return highest_submerged(cell_water, placed_water, cam_game.z);
    }
    let (h, wid) = match streamer.interior {
        Some(id) => {
            let c = world.index.cells.get(&id)?;
            let ws = &world.index.worldspaces[&world.world];
            let p = cam_game.to_array();
            let pw = &phys?.world;
            let h = pw.water_surface_for(p)?;
            // The placed volume that gave the surface (the highest one holding the
            // camera, as `water_surface_for` picks), and its base PWAT's water type.
            let placed = pw.placed_water().filter(|w| w.holds_below(p) && w.surface == h).map(|w| w.form_id).next().and_then(|r| {
                *placed_types.entry(r).or_insert_with(|| {
                    let base = sim?.refs.get(FormId(r))?.base;
                    fnv_world::sky::placeable_water_type(world.lo, world.schema, base.0)
                })
            });
            (h, fog_water_type(placed, c.water_type, ws.water)?)
        }
        None => dw.surface(world, cell_of(cam_game.x, cam_game.y))?,
    };
    (cam_game.z < h).then_some((h, wid))
}

fn highest_submerged(cell: Option<(f32, u32)>, placed: Option<(f32, u32)>, camera_z: f32) -> Option<(f32, u32)> {
    cell.into_iter().chain(placed).filter(|(height, _)| camera_z < *height).max_by(|a, b| a.0.total_cmp(&b.0))
}

/// Enhanced murk [hypothesis]: a camera deeper below the shallow/deep reference
/// planes sees a denser medium. Scale both under-fog planes, preserving the record's
/// entry appearance and its zero-distance share even when the near plane is negative.
/// A 2.5x ceiling keeps very deep views usable; malformed/narrow reference ranges
/// retain the existing fog rather than turning into an almost opaque medium.
fn depth_fog_range(above: [f32; 3], under: [f32; 3], depth: f32, enhanced: bool) -> (f32, f32) {
    let [near, far, _] = under;
    let far = far.max(near + 1.0);
    let reference = above[1] - above[0].max(0.0);
    if !enhanced || !above[0].is_finite() || !above[1].is_finite() || !reference.is_finite() || reference < 1.0 || !depth.is_finite() {
        return (near, far);
    }
    let density = (1.0 + depth.max(0.0) / reference).min(2.5);
    (near / density, far / density)
}

/// The water type an interior's under-water fog takes: the placed water's own (its PWAT's
/// DNAM "Water", the type the renderer draws that pool with, world.rs
/// `spawn_placed_water`), else the cell's (XCWT), else the worldspace's [hypothesis: the
/// fog matches the pool's look; e.g. Vault 34's pools hold toxic water, not the
/// worldspace's Potomac].
fn fog_water_type(placed: Option<u32>, cell: Option<u32>, worldspace: Option<u32>) -> Option<u32> {
    placed.or(cell).or(worldspace)
}

/// How far below the surface the camera sees the water's deep colour alone
/// [hypothesis, fitted: under Lake Mead the original's picture darkens evenly with the
/// player's depth and stops about 1,100 units down, the camera some 120 above the feet;
/// both waters measured store under fog near −1,000, which may be where it comes from].
pub const DEEP_AT: f32 = 1000.0;

/// The under-water colour as display bytes with the camera `depth` units below the
/// surface: the shallow colour at the surface, the deep colour from [`DEEP_AT`] down, in
/// proportion between [observed, base game and Tale of Two Wastelands, 2026-11-18,
/// world.md §6 "Under-water colour": Lake Mead at noon, feet 150 to 1,250 units down, (72, 81, 38) to
/// (36, 42, 15), the same whichever way the camera looks; the Pitt's river 180 down,
/// (46, 53, 33) for shallow (56, 60, 34)].
pub fn fog_bytes(shallow: [u8; 3], deep: [u8; 3], depth: f32) -> [u8; 3] {
    let t = if depth.is_finite() { (depth / DEEP_AT).clamp(0.0, 1.0) } else { 0.0 };
    std::array::from_fn(|i| (shallow[i] as f32 + (deep[i] as f32 - shallow[i] as f32) * t).round() as u8)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn underwater_fog(
    world: Res<WorldData>,
    streamer: Res<Streamer>,
    mut dw: ResMut<DistantWater>,
    sky: Res<SkyState>,
    mut cam: Query<(&GlobalTransform, &mut DistanceFog), crate::fly::WorldCamera>,
    phys: Option<Res<PhysState>>,
    sim: Option<Res<SimState>>,
    graphics: Option<Res<crate::graphics::Graphics>>,
    remaps: Option<Res<crate::water::WaterRemaps>>,
    submerged: Option<ResMut<CameraSubmerged>>,
    mut types: Local<WaterTypes>,
    mut placed_types: Local<HashMap<u32, Option<u32>>>,
    mut was_under: Local<bool>,
    screen: Option<Res<crate::imagespace::ScreenModifiers>>,
    tweaks: Option<Res<nvf_play::tweaks::TweakSettings>>,
) {
    let Ok((gt, mut fog)) = cam.single_mut() else { return };
    let enhanced = graphics.is_some_and(|g| g.water_effects);
    let camera = bevy_to_game(gt.translation());
    let under = water_above(&world, &streamer, &mut dw, phys.as_deref(), sim.as_deref(), &mut placed_types, camera, enhanced);
    let water = under.and_then(|(_, wid)| {
        let wid = remaps.as_ref().map_or(wid, |r| r.resolve(wid));
        types.0.entry(wid).or_insert_with(|| fnv_world::sky::water(world.lo, world.schema, wid)).clone()
    });
    // Under a water surface whether or not its water record could be read.
    if let Some(mut submerged) = submerged {
        if submerged.0 != under.is_some() {
            submerged.0 = under.is_some();
        }
    }
    let Some(w) = water else {
        if std::mem::take(&mut *was_under) {
            // Back above the surface: reapply the current weather/interior fog immediately
            // without forcing an unrelated sky refresh.
            info!("underwater: the camera is out of the water, the cell's fog again");
            let emissive = screen.as_deref().map_or(1.0, |s| s.hdr_ratio(fnv_world::imagespace::HDR_EMISSIVE_MULT));
            crate::sky::apply_cell_fog(&sky, &mut fog, emissive, tweaks.as_ref().is_some_and(|t| t.no_interior_fog));
        }
        return;
    };
    let depth = under.map_or(0.0, |(h, _)| h - camera.z);
    let colour = fog_bytes(w.shallow, w.deep, depth);
    if !std::mem::replace(&mut *was_under, true) {
        let h = under.map_or(0.0, |(h, _)| h);
        info!(
            "underwater: the camera is under water (surface {h:.1}), the water's under-water fog: colour ({}, {}, {}) {depth:.0} units down, between shallow ({}, {}, {}) and deep ({}, {}, {})",
            colour[0], colour[1], colour[2], w.shallow[0], w.shallow[1], w.shallow[2], w.deep[0], w.deep[1], w.deep[2]
        );
    }
    // Not lit: by day the original's colour is the same from 09:00 to 18:00, and its night
    // colour is the day's after the night's grade (world.md §6 "Under-water colour").
    // Water effects: the same rule without rounding to bytes, so a camera bobbing a few
    // units across one of the ~30 steps down to DEEP_AT does not flick the whole view
    // between two colours.
    let [r, g, b] = if enhanced { fog_linear(w.shallow, w.deep, depth) } else { colour.map(srgb_to_linear) };
    let (near, far) = depth_fog_range(w.above_fog, w.under_fog, depth, enhanced);
    fog.color = Color::LinearRgba(LinearRgba::rgb(r, g, b));
    fog.falloff = FogFalloff::Linear { start: near, end: far.max(near + 1.0) };
}

/// [`fog_bytes`]' colour unrounded, in linear light.
fn fog_linear(shallow: [u8; 3], deep: [u8; 3], depth: f32) -> [f32; 3] {
    let t = if depth.is_finite() { (depth / DEEP_AT).clamp(0.0, 1.0) } else { 0.0 };
    std::array::from_fn(|i| {
        let c = (shallow[i] as f32 + (deep[i] as f32 - shallow[i] as f32) * t) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    })
}

#[cfg(test)]
mod tests {
    use super::{DEEP_AT, depth_fog_range, fog_bytes, fog_linear, fog_water_type, highest_submerged};
    use fnv_world::sky::srgb_to_linear;

    /// Water effects' unrounded colour: the byte rule's colour where it lands on whole
    /// bytes, and no steps between them.
    #[test]
    fn unrounded_colour_matches_bytes_and_moves_smoothly() {
        let (shallow, deep) = ([52, 75, 52], [27, 39, 24]);
        for depth in [0.0, DEEP_AT, 5000.0, f32::NAN] {
            let bytes = fog_bytes(shallow, deep, depth).map(srgb_to_linear);
            let smooth = fog_linear(shallow, deep, depth);
            for i in 0..3 {
                assert!((bytes[i] - smooth[i]).abs() < 1e-6, "{depth}: {bytes:?} {smooth:?}");
            }
        }
        // A unit down changes the colour by far less than one byte step.
        let (a, b) = (fog_linear(shallow, deep, 400.0), fog_linear(shallow, deep, 401.0));
        assert!(a[1] > b[1] && a[1] - b[1] < 0.1 * (srgb_to_linear(60) - srgb_to_linear(59)));
    }

    #[test]
    fn murk_keeps_shallow_baseline_and_increases_with_descent() {
        let above = [-80.0, 850.0, 0.75];
        let under = [-2500.0, 5500.0, 1.0];
        assert_eq!(depth_fog_range(above, under, 0.0, true), (-2500.0, 5500.0));
        assert_eq!(depth_fog_range(above, under, 5000.0, false), (-2500.0, 5500.0));
        let share = |(near, far): (f32, f32), distance: f32| ((distance - near) / (far - near)).clamp(0.0, 1.0);
        let mut previous = share((-2500.0, 5500.0), 500.0);
        for depth in [10.0, 100.0, 500.0, 1000.0, 2000.0, 10000.0] {
            let range = depth_fog_range(above, under, depth, true);
            assert!((share(range, 0.0) - 0.3125).abs() < 1e-6);
            let next = share(range, 500.0);
            assert!(next >= previous);
            previous = next;
        }
        assert_eq!(depth_fog_range(above, under, 10000.0, true), (-1000.0, 2200.0));
    }

    #[test]
    fn malformed_reference_keeps_existing_fog() {
        for above in [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 0.001, 1.0], [0.0, f32::NAN, 1.0], [f32::NAN, 100.0, 1.0]] {
            assert_eq!(depth_fog_range(above, [0.0, 100.0, 1.0], 50.0, true), (0.0, 100.0));
        }
    }

    #[test]
    fn exterior_placed_volume_wins_only_while_submerged() {
        assert_eq!(highest_submerged(Some((10.0, 1)), Some((30.0, 2)), 20.0), Some((30.0, 2)));
        assert_eq!(highest_submerged(Some((10.0, 1)), Some((30.0, 2)), 30.0), None);
        assert_eq!(highest_submerged(Some((40.0, 1)), Some((30.0, 2)), 20.0), Some((40.0, 1)));
        assert_eq!(highest_submerged(None, Some((30.0, 2)), 20.0), Some((30.0, 2)));
    }

    /// #2916: the colour goes from shallow at the surface to deep at `DEEP_AT`, and stays.
    #[test]
    fn the_colour_goes_from_shallow_to_deep_with_depth() {
        // Potomac, the Mojave's water [observed: FalloutNV.esm WATR 0x00030009].
        let (shallow, deep) = ([52, 75, 52], [27, 39, 24]);
        assert_eq!(fog_bytes(shallow, deep, 0.0), shallow);
        assert_eq!(fog_bytes(shallow, deep, -50.0), shallow);
        assert_eq!(fog_bytes(shallow, deep, DEEP_AT), deep);
        assert_eq!(fog_bytes(shallow, deep, 5000.0), deep);
        assert_eq!(fog_bytes(shallow, deep, DEEP_AT / 2.0), [40, 57, 38]);
        // Evenly: equal steps down darken by equal amounts.
        let g = |d: f32| fog_bytes(shallow, deep, d)[1] as i32;
        assert_eq!(g(250.0) - g(500.0), g(500.0) - g(750.0));
        assert_eq!(fog_bytes(shallow, deep, f32::NAN), shallow);
    }

    #[test]
    fn placed_water_fogs_in_its_own_type() {
        // A PWAT's own water type wins over the cell's XCWT and the worldspace's water.
        assert_eq!(fog_water_type(Some(0x10), Some(0x20), Some(0x30)), Some(0x10));
        // A volume whose type cannot be read: the cell's, else the worldspace's.
        assert_eq!(fog_water_type(None, Some(0x20), Some(0x30)), Some(0x20));
        assert_eq!(fog_water_type(None, None, Some(0x30)), Some(0x30));
        assert_eq!(fog_water_type(None, None, None), None);
    }
}
