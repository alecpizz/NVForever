//! Real compiled Game writes -> production lighting reader -> SkyState -> Projection.
//! The source-only baseline adds a neutral reader seam, not a new lighting policy.
use super::*;
pub(super) use crate::authored_test_records as lighting_build;
#[path = "../../../crates/fnv-game/tests/common/lighting_templates.rs"]
pub(super) mod fixtures;
use fixtures::*;
use crate::world::{effective_cell_lighting, WorldData};
use fnv_script::vm::XVal;

fn world(f: &Fixture) -> WorldData {
    WorldData {
        lo: f.lo, schema: f.schema,
        index: Box::leak(Box::new(fnv_world::WorldIndex::build(f.lo, f.schema))),
        world: 0, persistent_by_grid: HashMap::new(),
        imposters: Box::leak(Box::new(fnv_world::imposter::ImposterRegions::build(f.lo, f.schema))),
        weather_regions: &[], dropped_plugins: Vec::new(),
    }
}

pub(super) fn sky() -> SkyState {
    SkyState {
        climate: Climate::default(), weather: Weather::default(), hour: 12.0,
        fog_override: None, interior: None, interior_grade: None, shadows: false,
        shadow_distance: DEFAULT_SHADOW_DISTANCE, shadow_reach: 1.0,
        shadow_layout: CascadeLayout::Quality, grade: true, time_scale: 0.0,
        clock_running: false, days_passed: 0.0, keep_once: false, prev: None, fade: 1.0,
        choices: Vec::new(), image_space: None, world_image_space_id: None, weather_fog_overrides: HashMap::new(), weather_visual_overrides: WeatherVisualOverrides::default(), adapters: HashMap::new(), changed: true,
        script: None, pinned: false, regions: Vec::new(), source: None, placed: false, region_cell: None,
        clock_seen: false, sun_hour: None, smooth_fade: false, fade_given: 0.0, clock_ahead: 0.0, rolled_for: None, pick: 0, rolls: 0,
        rng: DEFAULT_WEATHER_SEED, hour_index: None, live_globals: HashMap::new(),
        resume: None, flash: [0.0; 3], script_wind: None,
        moon_textures: Default::default(), lightning_request: false,
        moonlight: None, cloud_upgrade: None,
    }
}

fn app() -> (App, Entity, Entity) {
    let mut a = App::new();
    a.insert_resource(sky()).add_systems(Update, interior_clip);
    let main = a.world_mut().spawn((Projection::Perspective(PerspectiveProjection::default()),
        FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 })).id();
    let other = a.world_mut().spawn(Projection::Perspective(PerspectiveProjection { far: 12345.0, ..Default::default() })).id();
    (a, main, other)
}

fn far(a: &App, camera: Entity) -> f32 {
    match a.world().get::<Projection>(camera).unwrap() {
        Projection::Perspective(p) => p.far,
        _ => panic!("the authored camera must remain perspective"),
    }
}

fn apply(a: &mut App, w: &WorldData, game: Option<&fnv_game::Game<'_>>, cell: fnv_sim::FormId) {
    let lighting = effective_cell_lighting(w, game, cell.0);
    a.world_mut().resource_mut::<SkyState>().set_interior(lighting);
    a.update();
}

#[test]
fn cold_and_no_runtime_paths_keep_the_existing_real_camera_consumer() {
    let f = fixture(); let w = world(&f); let mut g = game(&f);
    let (mut a, main, other) = app();
    apply(&mut a, &w, None, CELL);
    assert_eq!(far(&a, main), 10000.0);
    apply(&mut a, &w, Some(&g), NO_TEMPLATE);
    assert_eq!(far(&a, main), 3000.0);
    apply(&mut a, &w, None, EMPTY_CELL);
    assert_eq!(far(&a, main), OUTDOOR_FAR, "the existing engine's no-positive-clip policy, not a native observation");
    set(&mut g, "SetCellLightingTemplate OwnedLightCell OwnedLightB\nSetLightingTemplateTraitNumeric OwnedLightB 15 30000");
    apply(&mut a, &w, None, CELL);
    assert_eq!(far(&a, main), 10000.0, "no runtime means the unchanged immutable reader");
    assert_eq!(far(&a, other), 12345.0, "an unrelated camera has no FlyCam marker");
}

#[test]
fn compiled_template_edits_reach_the_projection_at_cell_application_boundaries() {
    let f = fixture(); let w = world(&f); let mut g = game(&f);
    let (mut a, main, other) = app();
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), 10000.0);
    set(&mut g, "SetCellLightingTemplate OwnedLightCell OwnedLightB\nSetLightingTemplateTraitNumeric OwnedLightB 15 30000");
    assert_eq!(query(&mut g, "SetFunctionValue (GetLightingTemplateTraitNumeric OwnedLightB 15)"), XVal::Num(30000.0));
    a.update();
    assert_eq!(far(&a, main), 10000.0, "no implicit live-cell application is added to this fixture or policy");
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), 30000.0, "the actual Projection must receive the compiled session edit at an application boundary");
    apply(&mut a, &w, Some(&g), OTHER_CELL);
    assert_eq!(far(&a, main), 10000.0, "the other cell still selects its original template");
    set(&mut g, "SetLightingTemplateTraitNumeric OwnedLightB 15 0");
    a.update();
    assert_eq!(far(&a, main), 10000.0);
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), OUTDOOR_FAR, "zero follows the already documented engine fallback policy");
    set(&mut g, "SetCellFlag OwnedLightCell 15 0");
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), 1000.0, "the cell's own clip wins when inheritance is cleared");
    set(&mut g, "SetCellFlag OwnedLightCell 15 1\nSetCellLightingTemplate OwnedLightCell 0");
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), 1000.0, "explicitly removing the template does not resurrect the original");
    assert_eq!(far(&a, other), 12345.0);
}

#[test]
fn a_restored_game_and_runtime_absence_do_not_reuse_old_session_lighting() {
    let f = fixture(); let w = world(&f); let mut g = game(&f);
    let (mut a, main, other) = app();
    set(&mut g, "SetCellLightingTemplate OwnedLightCell OwnedLightB\nSetLightingTemplateTraitNumeric OwnedLightB 15 30000");
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), 30000.0);
    let loaded = restore(&f, &g.snapshot());
    apply(&mut a, &w, Some(&loaded), CELL);
    assert_eq!(far(&a, main), 10000.0, "a fresh Game::restore keeps the existing nonpersistent generic override policy");
    apply(&mut a, &w, Some(&g), CELL);
    assert_eq!(far(&a, main), 30000.0, "the prior Game's session remains separate");
    apply(&mut a, &w, None, CELL);
    assert_eq!(far(&a, main), 10000.0, "losing the runtime restores immutable cell lighting");
    assert_eq!(far(&a, other), 12345.0);
}

#[test]
fn cold_shifted_plugin_lighting_reaches_the_same_projection() {
    let f = shifted_fixture(); let w = world(&f); let g = game(&f);
    let (mut a, main, other) = app();
    apply(&mut a, &w, None, SHIFTED_CELL);
    assert_eq!(far(&a, main), 45000.0);
    apply(&mut a, &w, Some(&g), SHIFTED_CELL);
    assert_eq!(far(&a, main), 45000.0, "runtime availability must preserve the cold LoadOrder namespace mapping");
    assert_eq!(far(&a, other), 12345.0);
}

fn test_streamer(interior: Option<u32>) -> crate::world::Streamer {
    crate::world::Streamer {
        radius: 2,
        loaded: Default::default(),
        fades: Default::default(),
        fade_secs: 0.0,
        settled: true,
        arrival: None,
        per_frame: 1,
        spawning: Default::default(),
        dying: Default::default(),
        ahead: Default::default(),
        hide: Default::default(),
        interior,
        light_scale: 1.0,
        managed: Default::default(),
        manage_actors: false,
        stats: Default::default(),
        buffer: Default::default(),
        centre: None,
        waiting: Default::default(),
        model_overrides: Default::default(),
        coop_areas: Vec::new(),
        travels: 0,
    }
}

fn live_app(f: &Fixture, interior: Option<u32>) -> (App, Entity, Entity, Entity) {
    let mut a = App::new();
    a.add_message::<nvf_play::Travel>()
        .insert_resource(world(f))
        .insert_resource(test_streamer(interior))
        .insert_resource(nvf_play::script::ScriptState::over_plugins(f.lo.plugins))
        .insert_resource(sky())
        .insert_resource(Time::<()>::default())
        .insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<SkyMaterial>::default())
        .insert_resource(ClearColor::default())
        .insert_resource(GlobalAmbientLight::default())
        .add_systems(Update, (crate::world::cell_lighting, update_sky, interior_clip).chain());
    let main = a.world_mut().spawn((
        Transform::default(),
        DistanceFog::default(),
        Projection::Perspective(PerspectiveProjection::default()),
        FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 },
    )).id();
    let other = a.world_mut().spawn(Projection::Perspective(PerspectiveProjection { far: 12345.0, ..Default::default() })).id();
    let sun = a.world_mut().spawn((DirectionalLight::default(), Transform::default(), Sun)).id();
    (a, main, other, sun)
}

fn run_script(a: &mut App, lines: &str) {
    let mut script = a.world_mut().resource_mut::<nvf_play::script::ScriptState>();
    let g = &mut script.rt.as_mut().expect("script runtime present").game;
    set(g, lines);
}

fn fog_params(a: &App, camera: Entity) -> (Color, f32, f32) {
    let f = a.world().get::<DistanceFog>(camera).unwrap();
    let (near, far) = match &f.falloff {
        bevy::pbr::FogFalloff::Atmospheric { extinction, .. } => (extinction.x, extinction.y),
        bevy::pbr::FogFalloff::Linear { start, end } => (*start, *end),
        other => panic!("unexpected fog falloff variant: {other:?}"),
    };
    (f.color, near, far)
}

#[test]
fn live_current_cell_lighting_and_fog_refresh_without_travel() {
    let f = fixture();
    let (mut a, main, other, sun) = live_app(&f, Some(CELL.0));

    // Initial startup frame applies authored CELL + template A (ambient [1,2,3], clip 10000).
    a.update();
    assert_eq!(far(&a, main), 10000.0);
    let initial_ambient = a.world().resource::<GlobalAmbientLight>().color;
    let (initial_fog_color, initial_fog_near, initial_fog_far) = fog_params(&a, main);
    let initial_clear = a.world().resource::<ClearColor>().0;
    assert_eq!((initial_fog_near, initial_fog_far), (11.0, 22.0));

    // Unchanged frame performs no lighting update.
    a.update();
    assert!(!a.world().resource::<SkyState>().changed);
    assert_eq!(a.world().resource::<GlobalAmbientLight>().color, initial_ambient);

    // Live template swap + inheritance + ambient/directional/fog/clip edits while standing in CELL (no Travel!).
    run_script(
        &mut a,
        "SetCellLightingTemplate OwnedLightCell OwnedLightB\n\
         SetCellFlag OwnedLightCell 8 1\n\
         SetCellFlag OwnedLightCell 9 1\n\
         SetCellFlag OwnedLightCell 10 1\n\
         SetCellFlag OwnedLightCell 11 1\n\
         SetCellFlag OwnedLightCell 12 1\n\
         SetCellFlag OwnedLightCell 14 1\n\
         SetCellFlag OwnedLightCell 15 1\n\
         SetCellFlag OwnedLightCell 16 1\n\
         SetLightingTemplateTraitNumeric OwnedLightB 1 120\n\
         SetLightingTemplateTraitNumeric OwnedLightB 2 140\n\
         SetLightingTemplateTraitNumeric OwnedLightB 3 160\n\
         SetLightingTemplateTraitNumeric OwnedLightB 4 200\n\
         SetLightingTemplateTraitNumeric OwnedLightB 5 180\n\
         SetLightingTemplateTraitNumeric OwnedLightB 6 150\n\
         SetLightingTemplateTraitNumeric OwnedLightB 7 10\n\
         SetLightingTemplateTraitNumeric OwnedLightB 8 20\n\
         SetLightingTemplateTraitNumeric OwnedLightB 9 30\n\
         SetLightingTemplateTraitNumeric OwnedLightB 10 500\n\
         SetLightingTemplateTraitNumeric OwnedLightB 11 12000\n\
         SetLightingTemplateTraitNumeric OwnedLightB 14 0.8\n\
         SetLightingTemplateTraitNumeric OwnedLightB 15 30000\n\
         SetLightingTemplateTraitNumeric OwnedLightB 16 1.5",
    );
    a.update();

    let applied = a.world().resource::<SkyState>().interior.expect("interior lighting active");
    assert_eq!(applied.ambient, [120, 140, 160]);
    assert_eq!(applied.directional, [200, 180, 150]);
    assert_eq!(applied.fog_color, [10, 20, 30]);
    assert_eq!((applied.fog_near, applied.fog_far, applied.fog_power, applied.fog_clip_dist), (500.0, 12000.0, 1.5, 30000.0));
    assert_eq!(far(&a, main), 30000.0, "live fog clip distance reaches Projection without travel");
    assert_ne!(a.world().resource::<GlobalAmbientLight>().color, initial_ambient, "live ambient reaches GlobalAmbientLight");
    assert_ne!(a.world().resource::<ClearColor>().0, initial_clear, "live fog color reaches ClearColor");
    let (live_fog_color, live_fog_near, live_fog_far) = fog_params(&a, main);
    assert_ne!(live_fog_color, initial_fog_color, "live fog color reaches DistanceFog");
    assert_eq!((live_fog_near, live_fog_far), (500.0, 12000.0), "live fog distances reach DistanceFog");
    assert!(a.world().get::<DirectionalLight>(sun).unwrap().illuminance > 0.0);

    // Periodic dynamic ambient-only update (like a time-of-day interior ambient cycle) updates ambient live without moving clip/fog.
    let before_cycle_ambient = a.world().resource::<GlobalAmbientLight>().color;
    run_script(
        &mut a,
        "SetLightingTemplateTraitNumeric OwnedLightB 1 30\n\
         SetLightingTemplateTraitNumeric OwnedLightB 2 35\n\
         SetLightingTemplateTraitNumeric OwnedLightB 3 45",
    );
    a.update();
    assert_eq!(a.world().resource::<SkyState>().interior.unwrap().ambient, [30, 35, 45]);
    assert_ne!(a.world().resource::<GlobalAmbientLight>().color, before_cycle_ambient);
    let (_, cycle_fog_near, cycle_fog_far) = fog_params(&a, main);
    assert_eq!((cycle_fog_near, cycle_fog_far), (500.0, 12000.0));
    assert_eq!(far(&a, main), 30000.0);

    // Clearing inheritance and editing CELL XCLL directly via SetInteriorLightingTraitNumeric also refreshes live.
    run_script(
        &mut a,
        "SetCellFlag OwnedLightCell 8 0\n\
         SetCellFlag OwnedLightCell 15 0\n\
         SetInteriorLightingTraitNumeric OwnedLightCell 0 88\n\
         SetInteriorLightingTraitNumeric OwnedLightCell 1 77\n\
         SetInteriorLightingTraitNumeric OwnedLightCell 2 66\n\
         SetInteriorLightingTraitNumeric OwnedLightCell 15 16000",
    );
    a.update();
    assert_eq!(a.world().resource::<SkyState>().interior.unwrap().ambient, [88, 77, 66]);
    assert_eq!(far(&a, main), 16000.0);
    assert_eq!(far(&a, other), 12345.0, "unrelated camera remains untouched");
}

#[test]
fn live_refresh_ignores_unrelated_cell_edits_and_handles_runtime_loss_or_replacement() {
    let f = fixture();
    let (mut a, main, other, _sun) = live_app(&f, Some(CELL.0));

    a.update();
    assert_eq!(far(&a, main), 10000.0);

    // Editing an unrelated cell/template bumps lighting_revision once but leaves CELL's SkyState untouched.
    run_script(&mut a, "SetCellLightingTemplate OwnedOtherCell OwnedLightB\nSetLightingTemplateTraitNumeric OwnedLightB 15 44000");
    a.update();
    assert_eq!(far(&a, main), 10000.0);
    assert!(!a.world().resource::<SkyState>().changed);

    // Edit CELL live so clip becomes 30000.
    run_script(&mut a, "SetCellLightingTemplate OwnedLightCell OwnedLightB\nSetLightingTemplateTraitNumeric OwnedLightB 15 30000");
    a.update();
    assert_eq!(far(&a, main), 30000.0);

    // Replacing ScriptState with a fresh runtime that has the SAME lighting_revision count (only editing OTHER_CELL)
    // must still detect the new Game instance and revert CELL to authored 10000 without Travel.
    let mut fresh = nvf_play::script::ScriptState::over_plugins(f.lo.plugins);
    {
        let g = &mut fresh.rt.as_mut().unwrap().game;
        set(
            g,
            "SetCellLightingTemplate OwnedOtherCell OwnedLightB\n\
             SetLightingTemplateTraitNumeric OwnedLightB 15 44000\n\
             SetCellLightingTemplate OwnedOtherCell OwnedLightA\n\
             SetLightingTemplateTraitNumeric OwnedLightB 15 45000",
        );
    }
    let old_rev = a.world().resource::<nvf_play::script::ScriptState>().rt.as_ref().unwrap().game.lighting_token().1;
    let new_rev = fresh.rt.as_ref().unwrap().game.lighting_token().1;
    assert_eq!(old_rev, new_rev, "exercise identical revision numbers across distinct Game instances");
    *a.world_mut().resource_mut::<nvf_play::script::ScriptState>() = fresh;
    a.update();
    assert_eq!(far(&a, main), 10000.0, "distinct Game instance ID reverts CELL lighting immediately");

    // Re-apply live edit, then drop runtime completely -> reverts to immutable reader immediately.
    run_script(&mut a, "SetCellLightingTemplate OwnedLightCell OwnedLightB\nSetLightingTemplateTraitNumeric OwnedLightB 15 25000");
    a.update();
    assert_eq!(far(&a, main), 25000.0);
    a.world_mut().resource_mut::<nvf_play::script::ScriptState>().rt = None;
    a.update();
    assert_eq!(far(&a, main), 10000.0);
    assert_eq!(far(&a, other), 12345.0);
}
