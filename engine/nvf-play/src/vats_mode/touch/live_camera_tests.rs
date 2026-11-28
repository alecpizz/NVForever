// Production camera placement and transform propagation supplement the original
// static-camera tests. No camera transform is rewritten after fixture setup.
fn following_camera_scene(mode: Mode) -> (App, Entity, Frame) {
    let (mut app, pad, f) = scene(mode);
    app.world_mut().get_mut::<Camera>(f.camera).unwrap().computed.clip_from_view =
        PerspectiveProjection { fov: std::f32::consts::FRAC_PI_3, aspect_ratio: 1000. / 700., ..default() }.get_clip_from_view();
    let transform = app.world().get::<GlobalTransform>(f.camera).unwrap().compute_transform();
    app.world_mut().entity_mut(f.camera).insert(transform);
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<crate::control::BodyView>()
        .add_systems(Update, crate::control::place_camera
            .after(crate::control::read_controls).before(update));
    for _ in 0..40 { prepare(&mut app, f, true); app.update(); }
    let t = app.world().get::<Transform>(f.camera).unwrap();
    let global = app.world().get::<GlobalTransform>(f.camera).unwrap();
    assert!(global.affine().abs_diff_eq(t.compute_affine(), 0.0001));
    assert_eq!(app.world().resource::<VatsMode>().selected(), Some(A));
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    (app, pad, f)
}
fn live_view(app: &App) -> (f32, f32) {
    let p = app.world().resource::<Player>(); (p.heading, p.pitch)
}
fn real_ray_body(app: &mut App, f: Frame, p: Vec2) -> Option<(u32,u8)> {
    let ray = app.world().get::<Camera>(f.camera).unwrap()
        .viewport_to_world(app.world().get::<GlobalTransform>(f.camera).unwrap(), p).unwrap();
    let mut phys = app.world_mut().resource_mut::<PhysState>();
    let filter = shot_filter(&phys);
    match phys.ray(space::to_game(ray.origin), space::to_game(*ray.direction), 1000., filter)?.owner {
        Owner::Actor {form_id,part,..} => Some((form_id,part)), _ => None,
    }
}
#[test]
fn live_camera_tap_keeps_body_under_finger_until_release_then_follows_and_only_fire_accepts() {
    let (mut app, _, f) = following_camera_scene(Mode::Tap);
    let at = point(&app, f, B, 0); let before = live_view(&app);
    assert!(Rect::from_corners(Vec2::ZERO,f.logical).contains(at));
    assert_eq!(real_ray_body(&mut app, f, at), Some((B,0)));
    assert!(raw(&mut app, f, 1300, TouchPhase::Started, at, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().highlighted(), Some((B,Some(0))));
    assert_eq!(live_view(&app), before, "live selection cannot turn its own body away");
    for _ in 0..4 {
        prepare(&mut app, f, false); app.update();
        assert_eq!(real_ray_body(&mut app, f, at), Some((B,0)));
        assert_eq!(live_view(&app), before);
        assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    }
    assert!(raw(&mut app, f, 1300, TouchPhase::Ended, at, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(), [(FormId(B),Some(0))]);
    assert_ne!(live_view(&app), before, "ordinary target follow resumes on release");
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Marking);
    assert_eq!(app.world().resource::<VatsMode>().stats.shots, 0);
    prepare(&mut app, f, true);
    assert!(raw(&mut app, f, 1301, TouchPhase::Started, Vec2::new(900.,500.), true)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Firing);
}
#[test]
fn live_camera_hold_queues_once_then_cancel_restores_ordinary_follow() {
    let (mut app, _, f) = following_camera_scene(Mode::Hold);
    let at = point(&app, f, B, 1); let before = live_view(&app);
    assert_eq!(real_ray_body(&mut app, f, at), Some((B,1)));
    assert!(raw(&mut app, f, 1310, TouchPhase::Started, at, false)); app.update();
    for _ in 0..20 {
        prepare(&mut app, f, false); app.update();
        assert_eq!(live_view(&app), before);
        assert_eq!(real_ray_body(&mut app, f, at), Some((B,1)));
    }
    assert_eq!(app.world().resource::<State>().progress(), 1.);
    assert_eq!(app.world().resource::<VatsMode>().queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(), [(FormId(B),Some(1))]);
    assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Marking);
    assert_eq!(app.world().resource::<VatsMode>().stats.shots, 0);
    assert!(raw(&mut app, f, 1310, TouchPhase::Canceled, at, false)); app.update();
    assert_ne!(live_view(&app), before);
    assert_eq!(app.world().resource::<State>().progress(), 0.);
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 1, "no invented or repeated mark");
}
#[test]
fn live_camera_still_refuses_body_movement_away_from_current_ray() {
    let (mut app, _, f) = following_camera_scene(Mode::Hold);
    let at = point(&app, f, B, 0);
    assert!(raw(&mut app, f, 1320, TouchPhase::Started, at, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().highlighted(), Some((B,Some(0))));
    {
        let mut phys = app.world_mut().resource_mut::<PhysState>();
        phys.world.set_actor_parts(B,
            HitVolume {feet:[120.,10.,0.],radius:20.,height:160.,dead:false},
            &[PartCapsule {part:0,a:[120.,10.,130.],b:[120.,10.,143.],radius:8.},
              PartCapsule {part:1,a:[120.,10.,75.],b:[120.,10.,108.],radius:12.}]);
        phys.world.update_queries();
    }
    assert_ne!(real_ray_body(&mut app, f, at), Some((B,0)));
    prepare(&mut app, f, false); app.update();
    assert!(app.world().resource::<State>().highlighted().is_none());
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    assert!(raw(&mut app, f, 1320, TouchPhase::Ended, at, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(), 0);
    prepare(&mut app, f, true); let fresh = point(&app, f, B, 0);
    assert_eq!(real_ray_body(&mut app, f, fresh), Some((B,0)));
    assert!(raw(&mut app, f, 1321, TouchPhase::Started, fresh, false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().highlighted(), Some((B,Some(0))));
}
#[test]
fn following_camera_off_and_mark_and_fire_retain_explicit_existing_touch_policy() {
    for style in [VatsStyle::Classic,VatsStyle::DeadEye] {
        let (mut app, _, f) = following_camera_scene(Mode::Off);
        app.world_mut().resource_mut::<VatsMode>().style = style;
        prepare(&mut app, f, true); let at=point(&app,f,B,0);
        assert!(!raw(&mut app,f,1330,TouchPhase::Started,at,false)); app.update();
        assert_eq!(app.world().resource::<State>().mode(),Mode::Off);
        assert_eq!(app.world().resource::<VatsMode>().queue.len(),0);
    }
    // Enabled Mark and Fire now targets the actual body, with no Classic list.
    let (mut app, _, f)=following_camera_scene(Mode::Hold);
    {let mut v=app.world_mut().resource_mut::<VatsMode>();v.style=VatsStyle::DeadEye;v.classic=ClassicState::default();}
    prepare(&mut app,f,true); let at=point(&app,f,B,0);let before=live_view(&app);
    assert_eq!(real_ray_body(&mut app,f,at),Some((B,0)));
    assert!(raw(&mut app,f,1331,TouchPhase::Started,at,false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),0,"hold start only previews");
    assert!(raw(&mut app,f,1331,TouchPhase::Canceled,at,false));app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),0,"short canceled hold cannot queue");
    prepare(&mut app,f,true);
    assert!(raw(&mut app,f,1332,TouchPhase::Started,at,false));app.update();
    for _ in 0..20 {
        prepare(&mut app,f,false);app.update();
        assert_eq!(real_ray_body(&mut app,f,at),Some((B,0)));
        assert_eq!(live_view(&app),before,"Mark and Fire never adopts Classic camera follow");
    }
    let v=app.world().resource::<VatsMode>();
    assert_eq!(v.queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(),[(FormId(B),Some(0))]);
    assert_eq!(v.reserved(),20.0);assert_eq!(v.phase,Phase::Marking);assert_eq!(v.stats.shots,0);
    assert!(v.classic.targets.is_empty() && v.classic.selected.is_none());
    assert!(raw(&mut app,f,1332,TouchPhase::Ended,at,false));app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),1);
    assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Marking,"release does not confirm");
    prepare(&mut app,f,true);
    assert!(raw(&mut app,f,1333,TouchPhase::Started,Vec2::new(900.,500.),true));app.update();
    assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Firing,"a separate explicit Fire confirms");
}

#[test]
fn live_camera_target_refused_by_actual_sim_visibility_releases_contact_and_resumes_follow() {
    let (mut app, _, f) = following_camera_scene(Mode::Hold);
    let at = point(&app, f, B, 1); let before=live_view(&app);
    assert!(raw(&mut app, f, 1340, TouchPhase::Started, at, false)); app.update();
    assert_eq!(live_view(&app),before);
    assert_eq!(app.world().resource::<VatsMode>().highlighted(),Some((B,Some(1))));
    app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(B),false).unwrap();
    assert_eq!(real_ray_body(&mut app,f,at),Some((B,1)),"exercise actual late SimState refusal while collider remains");
    prepare(&mut app,f,false); app.update();
    assert!(app.world().resource::<State>().highlighted().is_none());
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),0);
    assert_ne!(live_view(&app),before,"refused contact must not hold automatic view follow");
    assert!(raw(&mut app,f,1340,TouchPhase::Ended,at,false)); app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),0);
}
