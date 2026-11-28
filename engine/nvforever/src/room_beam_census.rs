//! Developer census of window-ray beam candidates [design, #4698; docs/design/room-beams.md].

use crate::emittance::{EmittanceState, RoomRayEmittance};
use crate::sky::SkyState;
use crate::world::WorldData;
use bevy::math::Affine3A;
use bevy::prelude::*;
use nvf_assets::room_rays::{ray_shapes, RayBeam, RayShape};
use nvf_assets::GameData;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

const SWITCH: &str = "NVF_PLAY_DEBUG_ROOM_BEAM_CENSUS";
/// Let deferred model loads settle before reporting a batch [design].
const QUIET_FRAMES: u32 = 60;

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| crate::dev_aids::var(SWITCH).as_deref() == Some("1"))
}

pub(crate) fn install(app: &mut App) {
    if enabled() {
        app.init_resource::<Census>()
            .add_systems(Update, scan.after(crate::emittance::update_emittance));
    }
}

/// A placed shape with external emittance. The block identifies it even if names repeat.
#[derive(Component)]
pub(crate) struct RaySource {
    reference: u32,
    xemi: Option<u32>,
    path: String,
    block: u32,
    placed: Affine3A,
}

impl RaySource {
    pub(crate) fn new(reference: u32, xemi: Option<u32>, path: &str, block: u32, placed: Affine3A) -> Self {
        Self { reference, xemi, path: path.to_owned(), block, placed }
    }
}

#[derive(Resource, Default)]
struct Census {
    models: HashMap<String, Vec<RayShape>>,
    pending: BTreeMap<u32, CellCensus>,
}

#[derive(Default)]
struct CellCensus {
    name: String,
    quiet_frames: u32,
    candidates: usize,
    shafts: usize,
    rejected: BTreeMap<&'static str, usize>,
}

fn eligible(beam: Result<RayBeam, &'static str>, source: Option<RoomRayEmittance>) -> Result<RayBeam, &'static str> {
    match source {
        Some(RoomRayEmittance::Daylight) => (),
        Some(RoomRayEmittance::OtherRegion) => return Err("not a daylight region"),
        Some(RoomRayEmittance::Light) => return Err("light-backed XEMI"),
        None => return Err("unresolved XEMI"),
    }
    let beam = beam?;
    if !beam.is_long() {
        return Err("too short");
    }
    Ok(beam)
}

fn scan(
    world: Option<Res<WorldData>>,
    sky: Option<Res<SkyState>>,
    data: Option<Res<GameData>>,
    emittance: Res<EmittanceState>,
    mut census: ResMut<Census>,
    added: Query<&RaySource, Added<RaySource>>,
) {
    let (Some(world), Some(sky), Some(data)) = (world, sky, data) else { return };
    let Census { models, pending } = &mut *census;
    for ray in &added {
        let Some(cell) = world.index.refs.get(&ray.reference)
            .and_then(|r| world.index.cells.get(&r.cell))
            .filter(|c| c.is_interior()) else { continue };
        let shapes = models.entry(ray.path.clone()).or_insert_with(|| ray_shapes(&data, &ray.path));
        let Some(shape) = shapes.iter().find(|s| s.block == ray.block) else { continue };

        let result = ray.xemi.ok_or("no XEMI")
            .and_then(|xemi| eligible(shape.beam, emittance.room_ray_emittance(&sky, xemi)));
        let counts = pending.entry(cell.form_id).or_insert_with(|| CellCensus {
            name: cell.edid.clone().unwrap_or_default(),
            ..default()
        });
        counts.quiet_frames = 0;
        match result {
            Ok(beam) => {
                let shafts = shape.shafts.len().max(1);
                counts.candidates += 1;
                counts.shafts += shafts;
                debug!(
                    "room beam candidate: cell 0x{:08X}, ref 0x{:08X}, {} block {} {}: {shafts} shafts, origin {:?}, axis {:?}, length {:.1}, {:.0}% of edge length aligned",
                    cell.form_id, ray.reference, ray.path, ray.block, shape.name.as_deref().unwrap_or(""),
                    ray.placed.transform_point3(beam.a),
                    ray.placed.transform_vector3(beam.axis).normalize_or_zero(),
                    ray.placed.transform_vector3(beam.axis * beam.length).length(),
                    100.0 * beam.share,
                );
            }
            Err(reason) => {
                *counts.rejected.entry(reason).or_default() += 1;
            }
        }
    }

    pending.retain(|id, counts| {
        counts.quiet_frames += 1;
        if counts.quiet_frames < QUIET_FRAMES {
            return true;
        }
        let rejected: usize = counts.rejected.values().sum();
        info!(
            "room beam census: cell {} 0x{id:08X}: {} candidates, {} shafts, {rejected} rejected out of {} ray shapes, reasons {:?}",
            counts.name, counts.candidates, counts.shafts, counts.candidates + rejected, counts.rejected,
        );
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beam(length: f32) -> RayBeam {
        RayBeam {
            a: Vec3::ZERO,
            axis: Vec3::Z,
            length,
            u: Vec3::X,
            v: Vec3::Y,
            half_a: [10.0; 2],
            half_b: [10.0; 2],
            lean: [0.0; 2],
            share: 1.0,
        }
    }

    #[test]
    fn only_daylight_regions_qualify() {
        let shape = Ok(beam(100.0));
        assert_eq!(eligible(shape, Some(RoomRayEmittance::Daylight)), shape);
        assert_eq!(eligible(shape, Some(RoomRayEmittance::OtherRegion)), Err("not a daylight region"));
        assert_eq!(eligible(shape, Some(RoomRayEmittance::Light)), Err("light-backed XEMI"));
        assert_eq!(eligible(shape, None), Err("unresolved XEMI"));
    }

    #[test]
    fn fit_and_length_are_required_in_daylight() {
        let daylight = Some(RoomRayEmittance::Daylight);
        assert_eq!(eligible(Err("no edges"), daylight), Err("no edges"));
        assert_eq!(eligible(Ok(beam(20.0)), daylight), Err("too short"));
    }
}
