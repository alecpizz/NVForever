//! Authored water records through the real plane producer and camera fog plugin.
use super::*;
use crate::authored_test_records::{record, sub, tes4, top, z};
use crate::fly::FlyCam;
use crate::world::{game_to_bevy, ReadAhead, StreamStats};
use bevy::ecs::system::RunSystemOnce;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::pbr::DistanceFog;
use fnv_esm::{LoadOrder, Plugin as EsmPlugin, Schema};
use fnv_world::index::{Cell, Worldspace};
use nvf_assets::{GameData, ModelCache, NifMaterial, TextureCache};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const WATER: u32 = 0x800;
const WATER_REMAPPED: u32 = 0x801;

struct EmptyData(PathBuf);
impl Drop for EmptyData {
    fn drop(&mut self) {
        std::fs::remove_dir(&self.0).unwrap();
    }
}

fn setup() -> (App, Entity, EmptyData) {
    // WaterVisual's documented record fields, as decoded by schemas/fnv/sky.json.
    // No image files or installed records are needed by this authored water type.
    let mut visual = vec![0; 196];
    visual[44..48].copy_from_slice(&[20, 170, 210, 0]);
    visual[144..148].copy_from_slice(&7_f32.to_le_bytes());
    visual[148..152].copy_from_slice(&321_f32.to_le_bytes());
    let mut remapped_visual = vec![0; 196];
    remapped_visual[40..44].copy_from_slice(&[210, 40, 20, 0]);
    remapped_visual[44..48].copy_from_slice(&[190, 30, 15, 0]);
    remapped_visual[144..148].copy_from_slice(&11_f32.to_le_bytes());
    remapped_visual[148..152].copy_from_slice(&444_f32.to_le_bytes());
    let bytes = [
        tes4(),
        top(
            b"WATR",
            &[
                record(
                    b"WATR",
                    0,
                    WATER,
                    &[
                        sub(b"EDID", z("AuthoredLifecycleWater")),
                        sub(b"DNAM", visual),
                    ],
                ),
                record(
                    b"WATR",
                    0,
                    WATER_REMAPPED,
                    &[
                        sub(b"EDID", z("AuthoredRemappedWater")),
                        sub(b"DNAM", remapped_visual),
                    ],
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    let plugins = Box::leak(
        vec![EsmPlugin::parse(&bytes, "AuthoredLifecycle.esm".into()).unwrap()].into_boxed_slice(),
    );
    let lo = Box::leak(Box::new(LoadOrder::new(plugins).unwrap()));
    let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Box::leak(Box::new(Schema::load_dir(schemas.join("fnv")).unwrap()));
    let decoded = fnv_world::sky::water(lo, schema, WATER).unwrap();
    assert_eq!(decoded.deep, [20, 170, 210]);
    assert_eq!(decoded.under_fog, [7.0, 321.0, 0.0]);
    let mut index = fnv_world::WorldIndex::default();
    let mut ws = Worldspace {
        form_id: 1,
        water: Some(WATER),
        ..Default::default()
    };
    for (n, grid) in [(-1, 0), (0, 0), (1, 0), (4, 0)].into_iter().enumerate() {
        let id = 0x900 + n as u32;
        ws.grid.insert(grid, id);
        index.cells.insert(
            id,
            Cell {
                form_id: id,
                world: Some(1),
                grid: Some(grid),
                data_flags: 2,
                water_height: Some(100.0),
                water_type: Some(WATER),
                ..Default::default()
            },
        );
    }
    index.worldspaces.insert(1, ws);
    let world = WorldData {
        lo,
        schema,
        index: Box::leak(Box::new(index)),
        world: 1,
        persistent_by_grid: HashMap::new(),
        imposters: Box::leak(Box::new(fnv_world::imposter::ImposterRegions::build(
            lo, schema,
        ))),
        weather_regions: &[],
        dropped_plugins: Vec::new(),
    };
    let mut lod = LodState::new(&world, crate::lod::LodSettings::default(), true, None);
    // The chunks the old LOD square of radius 0 covered round (0, 0): cells -4..=3 by
    // 0..=3, so (-1, 0), (0, 0) and (1, 0), not (4, 0).
    lod.choose_for_test(vec![(4, -4, 0), (4, 0, 0)]);
    let streamer = Streamer {
        radius: 1,
        loaded: HashMap::new(),
        fades: HashMap::new(),
        fade_secs: 0.0,
        settled: true,
        arrival: None,
        per_frame: 1,
        spawning: HashMap::new(),
        dying: Vec::new(),
        ahead: ReadAhead::default(),
        hide: Vec::new(),
        interior: None,
        light_scale: 1.0,
        managed: HashSet::new(),
        manage_actors: false,
        stats: StreamStats::default(),
        buffer: Default::default(),
        centre: None,
        waiting: Vec::new(),
        model_overrides: HashMap::new(),
        coop_areas: Vec::new(),
        travels: 0,
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = EmptyData(std::env::temp_dir().join(format!(
        "nvf-water-lifecycle-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&dir.0).unwrap();
    let data = GameData::open_for_plugins(&dir.0, &[] as &[&str], &schemas.join("nif")).unwrap();
    let mut a = App::new();
    a.add_plugins(TransformPlugin)
        .insert_resource(world)
        .insert_resource(lod)
        .insert_resource(streamer)
        .insert_resource(data)
        .init_resource::<DistantWater>()
        .init_resource::<WaterCache>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<NifMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<WaterMaterial>>()
        .init_resource::<Assets<ShaderBuffer>>()
        .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
        .init_resource::<ModelCache>()
        .init_resource::<TextureCache>()
        // Whole budgets, as main's: `distant_water`'s surface reads follow the pace.
        .insert_resource(crate::stream_pace::StreamPace::new(crate::stream_pace::Mode::Off));
    a.world_mut().spawn((WorldRoot, game_to_bevy()));
    let camera = a
        .world_mut()
        .spawn((
            Camera3d::default(),
            FlyCam {
                yaw: 0.0,
                pitch: 0.0,
                speed: 0.0,
            },
            Transform::from_xyz(1024.0, 200.0, -1024.0),
            DistanceFog::default(),
        ))
        .id();
    (a, camera, dir)
}

fn local_plane(a: &mut App, grid: (i32, i32)) -> Entity {
    let cache = a.world().resource::<WaterCache>();
    let mesh = cache.plane.clone().unwrap();
    let mat = cache.materials[&(WATER, true, false, FlowDirection::STILL)].clone();
    let e = a
        .world_mut()
        .spawn((
            Name::new("Authored loaded cell water"),
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_xyz(
                grid.0 as f32 * CELL_SIZE + CELL_SIZE / 2.0,
                grid.1 as f32 * CELL_SIZE + CELL_SIZE / 2.0,
                100.0,
            ),
        ))
        .id();
    let root = {
        let w = a.world_mut();
        let mut roots = w.query_filtered::<Entity, With<WorldRoot>>();
        roots.single(w).unwrap()
    };
    a.world_mut().entity_mut(root).add_child(e);
    a.world_mut()
        .resource_mut::<Streamer>()
        .loaded
        .insert(grid, e);
    e
}

/// Advance `distant_water` until `main`'s budgeted surface reads (#2956) finish
/// populating the initial or newly expanded LOD square (`built_for.is_some()`).
fn settle_distant_water(a: &mut App) {
    for _ in 0..32 {
        a.update();
        if a.world().resource::<DistantWater>().built_for.is_some() {
            return;
        }
    }
    panic!("distant_water did not finish budgeted surface reads within 32 frames");
}

#[test]
fn equal_count_stream_replacement_removes_coplanar_distant_owner_and_retains_neighbors() {
    let (mut a, _, _dir) = setup();
    a.add_systems(Update, distant_water);
    settle_distant_water(&mut a);
    let local0 = local_plane(&mut a, (0, 0));
    a.update();
    let (old1, neighbor, material) = {
        let dw = a.world().resource::<DistantWater>();
        assert!(!dw.planes.contains_key(&(0, 0)));
        let neighbor = dw.planes[&(-1, 0)];
        (
            dw.planes[&(1, 0)],
            neighbor,
            a.world()
                .get::<MeshMaterial3d<WaterMaterial>>(neighbor)
                .unwrap()
                .0
                .clone(),
        )
    };
    a.world_mut()
        .resource_mut::<Streamer>()
        .loaded
        .remove(&(0, 0));
    a.world_mut().despawn(local0);
    let local1 = local_plane(&mut a, (1, 0));
    a.update();
    let dw = a.world().resource::<DistantWater>();
    assert!(
        !dw.planes.contains_key(&(1, 0)),
        "loaded replacement must evict its distant water even when loaded count is unchanged"
    );
    assert!(
        a.world().get_entity(old1).is_err(),
        "the old coplanar mesh must actually be despawned"
    );
    assert!(a.world().get::<Mesh3d>(local1).is_some());
    assert!(
        dw.planes.contains_key(&(0, 0)),
        "the unloaded water cell must regain distant coverage"
    );
    assert_eq!(dw.planes[&(-1, 0)], neighbor);
    assert_eq!(
        a.world()
            .get::<MeshMaterial3d<WaterMaterial>>(neighbor)
            .unwrap()
            .0,
        material
    );
    let before: HashMap<_, _> = dw.planes.clone();
    for _ in 0..8 {
        a.update();
    }
    assert_eq!(a.world().resource::<DistantWater>().planes, before);
    assert_eq!(a.world().resource::<Assets<WaterMaterial>>().len(), 1);
    assert_eq!(a.world().resource::<Assets<Mesh>>().len(), 1);
}

#[test]
fn lod_and_interior_changes_reconcile_real_planes_without_camera_or_count_changes() {
    let (mut a, _, _dir) = setup();
    a.add_systems(Update, distant_water);
    settle_distant_water(&mut a);
    let old: Vec<_> = a
        .world()
        .resource::<DistantWater>()
        .planes
        .values()
        .copied()
        .collect();
    assert_eq!(old.len(), 3);
    assert!(!a
        .world()
        .resource::<DistantWater>()
        .planes
        .contains_key(&(4, 0)));
    // A wider choice now also covers (4, 0).
    a.world_mut().resource_mut::<LodState>().choose_for_test(vec![(4, -4, 0), (4, 0, 0), (4, 4, 0)]);
    settle_distant_water(&mut a);
    assert!(
        a.world()
            .resource::<DistantWater>()
            .planes
            .contains_key(&(4, 0)),
        "a changed LOD chunk choice must update actual water coverage"
    );
    a.world_mut().resource_mut::<LodState>().enabled = false;
    a.update();
    assert!(
        a.world().resource::<DistantWater>().planes.is_empty(),
        "LOD off must remove old water meshes at the unchanged camera"
    );
    assert!(old.iter().all(|e| a.world().get_entity(*e).is_err()));
    a.world_mut().resource_mut::<LodState>().enabled = true;
    a.update();
    assert_eq!(a.world().resource::<DistantWater>().planes.len(), 4);
    a.world_mut().resource_mut::<Streamer>().interior = Some(0x999);
    a.update();
    assert!(a.world().resource::<DistantWater>().planes.is_empty());
    a.world_mut().resource_mut::<Streamer>().interior = None;
    a.update();
    assert_eq!(a.world().resource::<DistantWater>().planes.len(), 4);
}

fn fog_setup() -> (App, Entity, EmptyData) {
    let (mut a, camera, dir) = setup();
    let mut sky = crate::sky::authored_session_sky();
    sky.fog_override = Some((17.0, 4500.0));
    a.insert_resource(sky)
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 1.0,
            ..Default::default()
        })
        .add_plugins(crate::underwater::UnderwaterFogPlugin);
    (a, camera, dir)
}

fn assert_falloff(actual: &DistanceFog, expected: &DistanceFog) {
    assert_eq!(actual.color, expected.color);
    // FogFalloff's variants need not implement PartialEq; compare their complete public representation.
    assert_eq!(
        format!("{:?}", actual.falloff),
        format!("{:?}", expected.falloff)
    );
}

#[test]
fn current_camera_enter_exit_and_reentry_restore_current_weather_without_refresh_delay() {
    let (mut a, camera, _dir) = fog_setup();
    a.update();
    a.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .y = 50.0;
    a.update();
    let under = a.world().get::<DistanceFog>(camera).unwrap();
    assert!(
        matches!(
            under.falloff,
            bevy::pbr::FogFalloff::Linear {
                start: 7.0,
                end: 321.0
            }
        ),
        "current submerged Transform must select water fog in this frame"
    );
    assert!(under.color.to_linear().blue > under.color.to_linear().red);
    a.world_mut().resource_mut::<SkyState>().fog_override = Some((91.0, 12345.0));
    let sky = a.world().resource::<SkyState>();
    let rgb = sky.color(fnv_world::sky::ColorType::Fog);
    let expected = DistanceFog {
        color: Color::linear_rgba(rgb[0], rgb[1], rgb[2], 1.0),
        falloff: crate::fog::falloff(91.0, 12345.0, sky.fog_power(), true, sky.emissive_scale()),
        ..Default::default()
    };
    a.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .y = 200.0;
    a.update();
    assert_falloff(a.world().get::<DistanceFog>(camera).unwrap(), &expected);
    for _ in 0..4 {
        a.update();
    }
    assert_falloff(a.world().get::<DistanceFog>(camera).unwrap(), &expected);
    a.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .y = 50.0;
    a.update();
    assert!(matches!(
        a.world().get::<DistanceFog>(camera).unwrap().falloff,
        bevy::pbr::FogFalloff::Linear {
            start: 7.0,
            end: 321.0
        }
    ));
}

#[test]
fn leaving_water_into_an_interior_restores_current_lighting_and_preference() {
    let (mut a, camera, _dir) = fog_setup();
    a.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .y = 50.0;
    a.update();
    let light = fnv_world::lighting::CellLighting {
        fog_color: [190, 17, 61],
        fog_near: 27.0,
        fog_far: 1900.0,
        fog_power: 1.7,
        ..Default::default()
    };
    a.world_mut().resource_mut::<SkyState>().interior = Some(light);
    a.world_mut().resource_mut::<Streamer>().interior = Some(0x999);
    let tweaks = nvf_play::tweaks::TweakSettings {
        no_interior_fog: true,
        ..Default::default()
    };
    a.insert_resource(tweaks);
    let rgb = light.fog_color.map(srgb_to_linear);
    let expected = DistanceFog {
        color: Color::linear_rgb(rgb[0], rgb[1], rgb[2]),
        falloff: crate::fog::falloff(27.0, 1_000_000.0, 1.7, false, 1.0),
        ..Default::default()
    };
    a.update();
    assert_falloff(a.world().get::<DistanceFog>(camera).unwrap(), &expected);
    assert!(
        a.world()
            .get::<DistanceFog>(camera)
            .unwrap()
            .color
            .to_linear()
            .red
            > a.world()
                .get::<DistanceFog>(camera)
                .unwrap()
                .color
                .to_linear()
                .blue
    );
}

#[test]
fn worldspace_clear_resets_loaded_seen_and_rebuilds_planes() {
    let (mut a, _, _dir) = setup();
    a.add_systems(Update, distant_water);
    settle_distant_water(&mut a);
    let local0 = local_plane(&mut a, (0, 0));
    a.update();
    assert!(!a.world().resource::<DistantWater>().planes.contains_key(&(0, 0)));
    assert_eq!(a.world().resource::<DistantWater>().loaded_seen, vec![(0, 0)]);

    let old_planes: Vec<_> = a.world().resource::<DistantWater>().planes.values().copied().collect();
    a.world_mut().resource_mut::<Streamer>().loaded.remove(&(0, 0));
    a.world_mut().despawn(local0);
    a.world_mut().run_system_once(|mut commands: Commands, mut dw: ResMut<DistantWater>| {
        dw.clear(&mut commands);
    }).unwrap();
    let dw = a.world().resource::<DistantWater>();
    assert!(dw.planes.is_empty());
    assert!(dw.loaded_seen.is_empty());
    assert!(old_planes.iter().all(|e| a.world().get_entity(*e).is_err()));

    settle_distant_water(&mut a);
    let dw = a.world().resource::<DistantWater>();
    assert!(dw.planes.contains_key(&(0, 0)));
    assert_eq!(dw.planes.len(), 3);
}

#[test]
fn newly_spawned_water_material_gets_current_sky_in_same_frame() {
    let (mut a, _, _dir) = setup();
    let sky = crate::sky::authored_session_sky();
    let upper = Vec3::from_array(sky.color(ColorType::SkyUpper)).extend(1.0);
    let horizon = Vec3::from_array(sky.color(ColorType::Horizon)).extend(1.0);
    assert_ne!(upper, Vec4::ONE, "authored sky must differ from WaterParams placeholder Vec4::ONE");
    a.insert_resource(sky)
        .add_systems(Update, (distant_water, update_water_sky.after(distant_water)));
    settle_distant_water(&mut a);
    let cache = a.world().resource::<WaterCache>();
    let mats = a.world().resource::<Assets<WaterMaterial>>();
    assert_eq!(cache.materials.len(), 1);
    let mat = mats.get(cache.materials.values().next().unwrap()).unwrap();
    assert_eq!(mat.params.sky_upper, upper);
    assert_eq!(mat.params.sky_horizon, horizon);
}

#[test]
fn lens_camera_and_hdr_emissive_restore_sync_with_underwater_fog_in_same_frame() {
    let (mut a, camera, _dir) = fog_setup();
    let mut screen = crate::imagespace::ScreenModifiers::default();
    let mut graded_hdr = fnv_world::imagespace::Hdr::default();
    graded_hdr.v[fnv_world::imagespace::HDR_EMISSIVE_MULT] = 2.0;
    screen.set_hdr_for_test(fnv_world::imagespace::Hdr::default(), graded_hdr);
    a.insert_resource(screen)
        .add_systems(
            PostUpdate,
            crate::scope_view::sync_lens_fog.after(crate::underwater::underwater_fog),
        );
    let lens = a
        .world_mut()
        .spawn((crate::scope_view::LensCamera, DistanceFog::default()))
        .id();

    a.world_mut().get_mut::<Transform>(camera).unwrap().translation.y = 50.0;
    a.update();
    let main_under = a.world().get::<DistanceFog>(camera).unwrap().clone();
    let lens_under = a.world().get::<DistanceFog>(lens).unwrap().clone();
    assert!(matches!(
        main_under.falloff,
        bevy::pbr::FogFalloff::Linear { start: 7.0, end: 321.0 }
    ));
    assert_falloff(&lens_under, &main_under);

    a.world_mut().get_mut::<Transform>(camera).unwrap().translation.y = 200.0;
    a.update();
    let sky = a.world().resource::<SkyState>();
    let rgb = sky.color(fnv_world::sky::ColorType::Fog);
    let expected = DistanceFog {
        color: Color::linear_rgba(rgb[0], rgb[1], rgb[2], 1.0),
        falloff: crate::fog::falloff(17.0, 4500.0, sky.fog_power(), true, sky.emissive_scale() * 2.0),
        ..Default::default()
    };
    assert_falloff(a.world().get::<DistanceFog>(camera).unwrap(), &expected);
    assert_falloff(a.world().get::<DistanceFog>(lens).unwrap(), &expected);
}

#[test]
fn classic_mode_underwater_fog_honors_water_remaps() {
    let (mut a, camera, _dir) = fog_setup();
    a.insert_resource(WaterRemaps([(WATER, WATER_REMAPPED)].into_iter().collect()));
    a.world_mut().get_mut::<Transform>(camera).unwrap().translation.y = 50.0;
    a.update();
    let fog = a.world().get::<DistanceFog>(camera).unwrap();
    assert!(matches!(
        fog.falloff,
        bevy::pbr::FogFalloff::Linear { start: 11.0, end: 444.0 }
    ));
    assert!(fog.color.to_linear().red > fog.color.to_linear().blue);
}

#[test]
fn plugin_schedules_order_water_materials_and_auxiliary_camera_fog_handoffs() {
    use bevy::ecs::schedule::Schedules;
    let mut app = App::new();
    // Register consumers BEFORE producers so order comes strictly from schedule constraints.
    app.add_plugins((
        crate::scope_view::ScopeViewPlugin,
        crate::extra_camera::ExtraCameraPlugin,
        crate::reflection::ReflectionPlugin,
        crate::ripples::RipplesPlugin,
        crate::underwater::UnderwaterFogPlugin,
    ));
    add_update_systems(&mut app);

    let mut update = app.world_mut().resource_mut::<Schedules>().remove(Update).unwrap();
    update.initialize(app.world_mut()).expect("Update schedule builds");
    let u_names: Vec<String> = update.systems().expect("initialized").map(|(_, s)| s.name().to_string()).collect();
    let u_at = |n: &str| u_names.iter().position(|s| s.ends_with(n)).unwrap_or_else(|| panic!("{n} not in {u_names:?}"));
    assert!(u_at("water::distant_water") < u_at("water::update_water_sky"), "{u_names:?}");
    assert!(u_at("water::distant_water") < u_at("ripples::update_ripples"), "{u_names:?}");

    let mut post = app.world_mut().resource_mut::<Schedules>().remove(PostUpdate).unwrap();
    post.initialize(app.world_mut()).expect("PostUpdate schedule builds");
    let p_names: Vec<String> = post.systems().expect("initialized").map(|(_, s)| s.name().to_string()).collect();
    let p_at = |n: &str| p_names.iter().position(|s| s.ends_with(n)).unwrap_or_else(|| panic!("{n} not in {p_names:?}"));
    let fog = p_at("underwater::underwater_fog");
    assert!(fog < p_at("reflection::update_reflection"), "{p_names:?}");
    assert!(fog < p_at("scope_view::sync_lens_fog"), "{p_names:?}");
    assert!(fog < p_at("extra_camera::sync"), "{p_names:?}");
}
