// Real body projection -> routed raw contact -> VATS update/mark_actor.
// These controls intentionally start without any Classic selected target/list.
fn deadeye_scene(mode:Mode)->(App,Entity,Frame) {
    let fixture=scene_with_style(mode,VatsStyle::DeadEye);
    let v=fixture.0.world().resource::<VatsMode>();
    assert!(v.classic.targets.is_empty() && v.classic.selected.is_none());
    assert!(v.selected().is_none());
    fixture
}

#[test]
fn deadeye_off_reticle_tap_marks_exact_actor_part_and_only_separate_fire_accepts() {
    for (actor,part) in [(A,0),(A,1),(B,0),(B,1)] {
        let (mut app,_,f)=deadeye_scene(Mode::Tap);
        let before=ap(&mut app);let at=point(&app,f,actor,part);
        assert!((at.x-f.logical.x/2.0).abs()>40.0);
        let view={let p=app.world().resource::<Player>();(p.heading,p.pitch)};
        assert!(raw(&mut app,f,2000,TouchPhase::Started,at,false));app.update();
        assert_eq!(app.world().resource::<VatsMode>().highlighted(),Some((actor,Some(part))));
        assert!(app.world().resource::<VatsMode>().queue.is_empty());
        assert!(raw(&mut app,f,2000,TouchPhase::Ended,at,false));app.update();
        let v=app.world().resource::<VatsMode>();
        assert_eq!(v.queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(),[(FormId(actor),Some(part))]);
        assert_eq!(v.reserved(),20.0);assert_eq!(v.phase,Phase::Marking);assert_eq!(v.stats.shots,0);assert!(v.shot.is_none());
        assert!(v.classic.targets.is_empty() && v.classic.selected.is_none());
        assert_eq!(ap(&mut app),before);
        let p=app.world().resource::<Player>();assert_eq!((p.heading,p.pitch),view,"body touch cannot force the Mark-and-Fire camera");
        assert!(raw(&mut app,f,2000,TouchPhase::Ended,at,false));app.update();
        assert_eq!(app.world().resource::<VatsMode>().queue.len(),1);
        prepare(&mut app,f,true);
        assert!(raw(&mut app,f,2001,TouchPhase::Started,Vec2::new(900.,500.),true));app.update();
        assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Firing);
    }
}

#[test]
fn deadeye_hold_has_progress_and_queues_once_without_auto_fire_or_camera_follow() {
    let (mut app,_,f)=deadeye_scene(Mode::Hold);
    let at=point(&app,f,B,1);
    let view={let p=app.world().resource::<Player>();(p.heading,p.pitch)};
    raw(&mut app,f,2010,TouchPhase::Started,at,false);app.update();
    for _ in 0..3 {prepare(&mut app,f,false);app.update();}
    assert!(app.world().resource::<State>().progress()>0. && app.world().resource::<State>().progress()<1.);
    assert!(app.world().resource::<VatsMode>().queue.is_empty());
    for _ in 0..30 {prepare(&mut app,f,false);app.update();}
    let v=app.world().resource::<VatsMode>();
    assert_eq!(v.queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(),[(FormId(B),Some(1))]);
    assert_eq!(v.phase,Phase::Marking);assert_eq!(v.stats.shots,0);
    assert_eq!(v.highlighted(),Some((B,Some(1))));
    let p=app.world().resource::<Player>();assert_eq!((p.heading,p.pitch),view);
    raw(&mut app,f,2010,TouchPhase::Ended,at,false);app.update();
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),1);
    assert_eq!(app.world().resource::<State>().progress(),0.);
}

#[test]
fn deadeye_current_body_path_style_and_late_authority_changes_refuse_old_release() {
    for case in 0..9 {
        let (mut app,_,f)=deadeye_scene(Mode::Tap);let at=point(&app,f,B,0);
        raw(&mut app,f,2020,TouchPhase::Started,at,false);app.update();
        match case {
            0=>{raw(&mut app,f,2020,TouchPhase::Moved,Vec2::splat(2.),false);},
            1=>{raw(&mut app,f,2020,TouchPhase::Canceled,at,false);},
            2=>{
                let mut phys=app.world_mut().resource_mut::<PhysState>();
                let mut volume=phys.world.actor_volume(B).unwrap();volume.dead=true;phys.world.set_actor_volume(B,volume);
            },
            3=>{app.world_mut().resource_mut::<VatsMode>().style=VatsStyle::Classic;},
            4=>{app.world_mut().resource_mut::<crate::pause::Paused>().0=true;},
            5=>{app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(B),false).unwrap();},
            6=>{app.world_mut().get_mut::<Camera>(f.camera).unwrap().computed.target_info=None;},
            7=>{app.world_mut().resource_mut::<Bindings>().device=InputDevice::Controller;},
            _=>{app.world_mut().resource_scope(|world,mut sim:Mut<SimState>| {
                let mut script=world.resource_mut::<ScriptState>();
                with_world(script.rt.as_mut().unwrap(),&mut sim,|rt|fnv_game::functions::set_actor_value_current(&mut rt.game,FormId(B),fnv_game::detection::INVISIBILITY,1.).unwrap());
            });},
        }
        raw(&mut app,f,2020,TouchPhase::Ended,at,false);app.update();
        assert!(app.world().resource::<VatsMode>().queue.is_empty(),"case {case}");
        assert_eq!(app.world().resource::<VatsMode>().stats.shots,0);
        app.world_mut().run_system_once(|mut b:Bridge|b.cancel()).unwrap();
        assert!(raw(&mut app,f,2020,TouchPhase::Ended,at,false),"retired tail {case}");
    }
}

#[test]
fn deadeye_empty_fire_ap_and_ammo_limits_keep_reservations_and_no_attacks() {
    let (mut app,_,f)=deadeye_scene(Mode::Tap);
    raw(&mut app,f,2030,TouchPhase::Started,Vec2::new(900.,500.),true);app.update();
    assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Marking);
    raw(&mut app,f,2030,TouchPhase::Ended,Vec2::new(900.,500.),true);
    app.world_mut().resource_scope(|world,mut sim:Mut<SimState>| {
        let mut script=world.resource_mut::<ScriptState>();
        with_world(script.rt.as_mut().unwrap(),&mut sim,|rt|fnv_game::functions::set_actor_value_current(&mut rt.game,FormId::PLAYER_REF,ACTION_POINTS,30.).unwrap());
    });
    tap(&mut app,f,2031,A,0);tap(&mut app,f,2032,B,1);
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),1);
    assert_eq!(app.world().resource::<VatsMode>().reserved(),20.);assert_eq!(ap(&mut app).0,30.);
    let (mut app,_,f)=deadeye_scene(Mode::Tap);
    for id in 2033..2036 {tap(&mut app,f,id,B,0);}
    assert_eq!(app.world().resource::<VatsMode>().queue.len(),2);
    assert_eq!(app.world().resource::<VatsMode>().reserved(),40.);
    assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Marking);
    assert_eq!(app.world().resource::<VatsMode>().stats.shots,0);
}

#[test]
fn deadeye_explicit_off_preserves_native_reticle_mark_and_confirm_for_mouse_and_pad() {
    assert_eq!(Mode::default(),Mode::Off);assert_eq!(VatsStyle::default(),VatsStyle::Classic);
    for device in [InputDevice::KeyboardMouse,InputDevice::Controller] {
        let (mut app,pad,f)=deadeye_scene(Mode::Off);
        let at=point(&app,f,B,1);assert!(!raw(&mut app,f,2040,TouchPhase::Started,at,false));
        let target=app.world().resource::<PhysState>().world.actor_part_center(A,0).unwrap();
        let eye=app.world().resource::<Player>().eye();let delta=Vec3::from_array(target)-Vec3::from_array(eye);
        {let mut p=app.world_mut().resource_mut::<Player>();p.heading=delta.x.atan2(delta.y);p.pitch=delta.z.atan2(delta.truncate().length());}
        crate::bindings::set_device(app.world_mut(),device);app.update();
        crate::pad::device::tests::control_button(&mut app,pad,device,Control::Use,true);app.update();
        assert_eq!(app.world().resource::<VatsMode>().queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(),[(FormId(A),Some(0))]);
        crate::pad::device::tests::control_button(&mut app,pad,device,Control::Use,false);app.update();
        crate::pad::device::tests::control_button(&mut app,pad,device,Control::Activate,true);app.update();
        assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Firing);
        assert_eq!(app.world().resource::<State>().mode(),Mode::Off);
    }
}

#[test]
fn classic_contact_cannot_cross_into_deadeye_but_a_fresh_contact_can_mark() {
    let (mut app,_,f)=scene(Mode::Tap);let at=point(&app,f,B,1);
    raw(&mut app,f,2050,TouchPhase::Started,at,false);app.update();
    {let mut v=app.world_mut().resource_mut::<VatsMode>();v.style=VatsStyle::DeadEye;v.classic=ClassicState::default();}
    raw(&mut app,f,2050,TouchPhase::Ended,at,false);app.update();
    assert!(app.world().resource::<VatsMode>().queue.is_empty(),"style identity retires the already-admitted Classic request");
    prepare(&mut app,f,true);
    tap(&mut app,f,2051,B,1);
    assert_eq!(app.world().resource::<VatsMode>().queue.marks().iter().map(|m|(m.target,m.part)).collect::<Vec<_>>(),[(FormId(B),Some(1))]);
    assert_eq!(app.world().resource::<VatsMode>().phase,Phase::Marking);
}
