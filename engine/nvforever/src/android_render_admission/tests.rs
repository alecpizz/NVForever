use super::*;
use bevy::ecs::schedule::{ScheduleLabel, SystemSet};

fn streamer() -> crate::world::Streamer {
    crate::world::Streamer { radius: 0, loaded: default(), fades: default(), fade_secs: 0.0,
        settled: true, arrival: None, per_frame: 1, spawning: default(), dying: default(),
        ahead: default(), hide: default(), interior: None, light_scale: 1.0, managed: default(),
        manage_actors: false, stats: default(), buffer: default(), centre: None,
        waiting: default(), model_overrides: default(), coop_areas: Vec::new(), travels: 0 }
}
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin, AndroidRenderAdmissionPlugin));
    app.init_state::<PlayState>().insert_resource(Gate(true)).insert_resource(streamer())
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<NifMaterial>>();
    app.world_mut().spawn(nvf_play::PlayerCamera);
    app
}
fn spawn(app: &mut App, skin: bool, visible: bool) -> Entity {
    let visibility = if visible { ViewVisibility::VISIBLE } else { ViewVisibility::HIDDEN };
    let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
    let material = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(nvf_assets::nif_material::nif_material(StandardMaterial {
        base_color_texture: Some(Handle::default()), ..default()
    }));
    let mut entity = app.world_mut().spawn((Mesh3d(mesh), MeshMaterial3d(material), visibility));
    if skin { entity.insert(SkinnedMesh { inverse_bindposes: default(), joints: vec![] }); }
    entity.id()
}
#[test]
fn apk_debug_gate_is_exact_and_independent_of_native_release_profile() {
    assert!(diagnostics_metadata(Some("debug"), Some("enabled")));
    for value in [None, Some("release"), Some("DEBUG"), Some("debug\n"), Some("")] {
        assert!(!diagnostics_metadata(value, Some("enabled")));
    }
    for value in [None, Some("disabled"), Some("ENABLED"), Some("enabled\n"), Some("")] {
        assert!(!diagnostics_metadata(Some("debug"), value));
    }
    let mut app = app();
    spawn(&mut app, true, true);
    app.insert_resource(Gate(false));
    app.update();
    assert!(app.world().resource::<Snapshot>().ticket.is_none());
    assert_eq!(app.world().resource::<Sampler>().bursts, 0);
}
#[test]
fn actual_app_caps_visible_samples_deterministically_and_skips_hidden_shapes() {
    let mut app = app();
    for _ in 0..200 { spawn(&mut app, true, true); }
    for _ in 0..100 { spawn(&mut app, false, true); }
    let hidden = spawn(&mut app, true, false);
    app.update();
    let snapshot = app.world().resource::<Snapshot>();
    assert_eq!((snapshot.skin_total, snapshot.rigid_total), (200, 100));
    assert_eq!((snapshot.skins.len(), snapshot.rigid.len()), (128, 32));
    assert!(snapshot.skins.iter().all(|s| s.entity != MainEntity::from(hidden)));
    assert!(snapshot.skins.iter().all(|s| s.cpu_material && s.diffuse.is_some()));
    let before: Vec<_> = snapshot.skins.iter().map(|s| s.entity).collect();
    app.update();
    assert_eq!(before, app.world().resource::<Snapshot>().skins.iter().map(|s| s.entity).collect::<Vec<_>>());
}
#[test]
fn actual_app_sparse_bursts_survive_lifecycle_without_resetting_process_budget() {
    let mut app = app();
    spawn(&mut app, true, true);
    let mut offsets = Vec::new();
    for frame in 0..65 {
        app.update();
        if let Some(ticket) = app.world().resource::<Snapshot>().ticket {
            assert_eq!(ticket.burst, 1);
            assert_eq!(ticket.trigger, Trigger::Load);
            offsets.push(frame);
        }
    }
    assert_eq!(offsets, OFFSETS);
    assert_eq!(app.world().resource::<Sampler>().captures, 8,
        "active non-sample frames perform no shape/material capture");
    assert_eq!(app.world().resource::<Sampler>().id_scans, 0,
        "active bursts need no separate new-skin detection scans");
    spawn(&mut app, true, true);
    for _ in 1..IDLE_DETECTION_FRAMES {
        app.update();
        assert!(app.world().resource::<Snapshot>().ticket.is_none());
        assert_eq!(app.world().resource::<Sampler>().id_scans, 0);
        assert_eq!(app.world().resource::<Sampler>().captures, 8);
    }
    app.update();
    assert_eq!(app.world().resource::<Snapshot>().ticket.unwrap().trigger, Trigger::NewSkin);
    assert_eq!(app.world().resource::<Sampler>().id_scans, 1);
    assert_eq!(app.world().resource::<Sampler>().captures, 9);
    app.world_mut().write_message(AppLifecycle::Suspended);
    app.update();
    assert!(app.world().resource::<Snapshot>().ticket.is_none());
    app.world_mut().write_message(AppLifecycle::Running);
    app.update();
    assert_eq!(app.world().resource::<Snapshot>().ticket.unwrap().trigger, Trigger::Resume);
    assert_eq!(app.world().resource::<Sampler>().bursts, 3);
    assert_eq!(app.world().resource::<Sampler>().captures, 10,
        "resume captures immediately rather than waiting for idle detection");
    for _ in 0..12 {
        app.world_mut().write_message(AppLifecycle::Suspended); app.update();
        app.world_mut().write_message(AppLifecycle::Running); app.update();
    }
    assert_eq!(app.world().resource::<Sampler>().bursts, BURSTS);
    for _ in 0..66 { app.update(); }
    assert!(app.world().resource::<Sampler>().active.is_none());
    spawn(&mut app, true, true);
    app.update();
    assert!(app.world().resource::<Snapshot>().ticket.is_none(), "new shapes cannot reset the process ceiling");
    let sampler = app.world().resource::<Sampler>();
    let completed_work = (sampler.captures, sampler.id_scans);
    for _ in 0..16 { app.update(); }
    let sampler = app.world().resource::<Sampler>();
    assert_eq!((sampler.captures, sampler.id_scans), completed_work,
        "exhausted process budget performs no further shape or idle detection scans");
    let mut observed = Observed::default();
    let ticket = Some(Ticket { burst: 1, trigger: Trigger::Load, sample: 0 });
    assert!(observed.admit(ticket));
    assert!(!observed.admit(ticket));
    assert!(!observed.admit(None));
    assert!(!observed.admit(ticket), "missing extract cannot replay an already observed sample");
}
#[test]
fn actual_extract_copies_bounded_snapshots_and_clears_stale_tickets_after_loading() {
    #[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Fixture { Apply, Despawn }
    let mut app = app();
    app.add_plugins(bevy::render::extract_plugin::ExtractPlugin::<RenderApp>::new(
        |_, _| {}, || Schedule::new(Render), Render.intern(), Fixture::Apply.intern(), Fixture::Despawn.intern()));
    // Use the same production finish/extract path, with no RenderPlugin/GPU device.
    AndroidRenderAdmissionPlugin.finish(&mut app);
    let watched = MainEntity::from(spawn(&mut app, true, true));
    let mut foreign = bevy::pbr::MaterialQueueProbe::default();
    foreign.watched.insert(watched);
    app.get_sub_app_mut(RenderApp).unwrap().world_mut().insert_resource(foreign);
    app.update();
    let render = app.get_sub_app(RenderApp).unwrap();
    assert_eq!(render.world().resource::<Snapshot>().skins.len(), 1);
    assert_eq!(render.world().resource::<Snapshot>().ticket, app.world().resource::<Snapshot>().ticket);
    assert_eq!(render.world().resource::<bevy::pbr::MaterialQueueProbe>().watched.len(), 1);
    assert!(render.world().resource::<bevy::pbr::MaterialQueueProbe>().watched.contains(&watched),
        "extract cannot take ownership of the Pip-Boy queue observer");
    app.world_mut().resource_mut::<crate::world::Streamer>().settled = false;
    app.update();
    assert!(app.get_sub_app(RenderApp).unwrap().world().resource::<Snapshot>().ticket.is_none());
    assert_eq!(app.world().resource::<Sampler>().bursts, 1);
    app.world_mut().resource_mut::<crate::world::Streamer>().settled = true;
    app.update();
    assert_eq!(app.get_sub_app(RenderApp).unwrap().world().resource::<Snapshot>().ticket.unwrap().burst, 2);
    assert!(app.get_sub_app(RenderApp).unwrap().world().resource::<bevy::pbr::MaterialQueueProbe>().watched.contains(&watched));
}
#[test]
fn numeric_schema_keeps_cpu_gpu_and_stale_instance_evidence_distinct() {
    let mut app = app();
    spawn(&mut app, true, true);
    app.update();
    let snapshot = app.world().resource::<Snapshot>();
    let full = Flags { mesh: true, slab: true, instance: true, diffuse: true, prepared: true, bound: true,
        main: true, main_ready: true, prepass: true, prepass_ready: true };
    let all = count(&snapshot.skins, |_| full);
    let none = Counts::default();
    assert_eq!(line(snapshot, &all, &none) + "\n", include_str!("../../../../android/tests/fixtures/render-admission-line.txt").replace("\r\n", "\n"));
    let waiting = count(&snapshot.skins, |_| Flags { mesh: true, slab: true, ..default() });
    assert_eq!((waiting.cpu_material, waiting.diffuse_refs, waiting.mesh), (1, 1, 1));
    assert_eq!((waiting.diffuse, waiting.prepared, waiting.bound, waiting.main_ready), (0, 0, 0, 0));
    let stale = count(&snapshot.skins, |_| Flags { instance: false, ..full });
    assert_eq!((stale.instance, stale.prepared, stale.main_ready), (0, 1, 1));
    assert!(!line(snapshot, &stale, &none).contains("queue"));
    assert!(!line(snapshot, &stale, &none).contains("submitted"));
    assert!(!line(snapshot, &stale, &none).contains("pixels"));
}
