use super::*;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{CascadesVisibleEntities, VisibleMeshEntities};
use bevy::ecs::system::RunSystemOnce;
use bevy::light::cascade::{Cascade, Cascades};

#[test]
fn precomputed_outline_edges_match_direct_edge_overlap_checks() {
    let poly = hull([
        Vec2::new(-10.0, -5.0),
        Vec2::new(12.0, -3.0),
        Vec2::new(8.0, 9.0),
        Vec2::new(-7.0, 11.0),
        Vec2::new(0.0, 2.0),
    ]);
    let edges = outline_edges(&poly);
    let direct = |outline: &[Vec2], centre: Vec2, reach: Vec2| -> bool {
        (0..outline.len()).all(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            let n = (-(b - a).perp()).normalize_or_zero();
            n.dot(centre - a) - n.abs().dot(reach) <= 0.0
        })
    };
    for ix in -20..=20 {
        for iy in -20..=20 {
            let c = Vec2::new(ix as f32 * 0.9, iy as f32 * 0.9);
            for r in [Vec2::ZERO, Vec2::new(1.5, 0.7), Vec2::splat(4.0)] {
                assert_eq!(overlaps_edges(&edges, c, r), direct(&poly, c, r), "c={c:?} r={r:?}");
            }
        }
    }
}

#[test]
fn sun_casters_over_slice_skips_shared_mirror_and_held_cascades() {
    let mut world = World::new();
    let main_cam = world.spawn((GlobalTransform::IDENTITY, Projection::Perspective(PerspectiveProjection::default()))).id();
    let mirror_cam = world.spawn((GlobalTransform::IDENTITY, Projection::Perspective(PerspectiveProjection::default()))).id();
    // Caster far outside the view frustum so slice culling drops it whenever the cascade runs.
    let far_caster = world.spawn((
        Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
        GlobalTransform::from_xyz(50_000.0, 50_000.0, 0.0),
    )).id();

    let mut visible = CascadesVisibleEntities::default();
    visible.entities.insert(main_cam, vec![
        VisibleMeshEntities { entities: vec![far_caster] },
        VisibleMeshEntities { entities: vec![far_caster] },
    ]);
    visible.entities.insert(mirror_cam, vec![
        VisibleMeshEntities { entities: vec![far_caster] },
        VisibleMeshEntities { entities: vec![far_caster] },
    ]);
    let mut cascades = Cascades::default();
    let dummy_cascade = || Cascade {
        world_from_cascade: Mat4::IDENTITY,
        clip_from_cascade: Mat4::IDENTITY,
        clip_from_world: Mat4::IDENTITY,
        texel_size: 1.0,
    };
    cascades.cascades.insert(main_cam, vec![dummy_cascade(), dummy_cascade()]);
    cascades.cascades.insert(mirror_cam, vec![dummy_cascade(), dummy_cascade()]);
    let config = CascadeShadowConfig { bounds: vec![100.0, 400.0], overlap_proportion: 0.2, minimum_distance: 5.0 };
    let sun = world.spawn((Sun, visible, cascades, config, GlobalTransform::IDENTITY)).id();

    world.insert_resource(crate::shadow_rate::MirrorShadowShare(Some((main_cam, mirror_cam))));
    // Hold cascade 1, leave cascade 0 active.
    world.insert_resource(bevy::pbr::HoldDirectionalShadowMaps(1u32 << 1));

    world.run_system_once(sun_casters_over_slice).unwrap();

    let vis = world.get::<CascadesVisibleEntities>(sun).unwrap();
    // Main camera cascade 0 ran and culled the far caster; cascade 1 was held and untouched.
    assert!(vis.entities[&main_cam][0].entities.is_empty(), "active cascade 0 must cull far caster");
    assert_eq!(vis.entities[&main_cam][1].entities, vec![far_caster], "held cascade 1 must be skipped");
    // Shared mirror camera was skipped entirely.
    assert_eq!(vis.entities[&mirror_cam][0].entities, vec![far_caster], "shared mirror view must be skipped");
    assert_eq!(vis.entities[&mirror_cam][1].entities, vec![far_caster]);
}
