// Actual posed-model and optional-dismissal painters, core recolour, registered
// settings and existing Taffy geometry; no direct colour writes or fake UI edges.
fn pip_palette_child(selector:&str)->bool {
    const CHILD:&str="NVF_TEST_PIPBOY_PALETTE_CHILD";
    if std::env::var_os(CHILD).is_some() {return false;}
    // Process-global HUD/PipBoy colours require isolation from parallel tests.
    let mut child=std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact",selector,"--test-threads=1"]).env(CHILD,"1").spawn().unwrap();
    let started=std::time::Instant::now();
    let status=loop {
        match child.try_wait() {
            Ok(Some(status))=>break status,
            Ok(None) if started.elapsed()<std::time::Duration::from_secs(30)=>std::thread::sleep(std::time::Duration::from_millis(10)),
            Ok(None)=>{let killed=child.kill();let reaped=child.wait();panic!("isolated PipBoy palette control exceeded 30s; own-child kill={killed:?}, reap={reaped:?}");}
            Err(error)=>{let killed=child.kill();let reaped=child.wait();panic!("isolated PipBoy palette wait failed: {error}; own-child kill={killed:?}, reap={reaped:?}");}
        }
    };
    assert!(status.success(),"isolated actual PipBoy palette control failed");
    true
}
fn pip_palette_rgb(app:&mut App,pipboy:bool,rgb:[u8;3]) {
    let keys=if pipboy {["pipboy-red","pipboy-green","pipboy-blue"]}else{["hud-red","hud-green","hud-blue"]};
    for (key,value) in keys.into_iter().zip(rgb) {
        let row=app.world().resource::<nvf_play::settings::Settings>().index(key).unwrap();
        nvf_play::settings::set_slider(app.world_mut(),row,f32::from(value)/255.);
    }
}
fn pip_palette_caption(app:&mut App,label:&str)->Entity {
    let world=app.world_mut();let mut q=world.query::<(Entity,&Text)>();
    let candidates:Vec<_>=q.iter(world).filter(|(_,text)|text.0==label).map(|(entity,_)|entity).collect();
    assert_eq!(candidates.len(),1,"actual overlay caption is unique");candidates[0]
}
fn pip_palette_assert(app:&mut App,targets:&[(Entity,Node,Rect,Entity,Entity,f32)],shown:bool) {
    let ink=nvf_play::interface::pipboy_amber(255.0);
    for (entity,node,rect,camera,caption,border_alpha) in targets {
        assert!(app.world().get::<Node>(*entity).unwrap()==node,"palette or mask cannot alter the hit-layout Node");
        let computed=app.world().get::<bevy::ui::ComputedNode>(*entity).unwrap();
        let transform=app.world().get::<bevy::ui::UiGlobalTransform>(*entity).unwrap();
        assert_eq!(Rect::from_center_size(transform.translation,computed.size()),*rect);
        assert_eq!(app.world().get::<bevy::ui::ComputedUiTargetCamera>(*entity).unwrap().get(),Some(*camera));
        let alpha=if shown {1.0}else{0.0};
        assert_eq!(app.world().get::<TextColor>(*caption).unwrap().0,ink.with_alpha(alpha));
        assert_eq!(*app.world().get::<BorderColor>(*entity).unwrap(),BorderColor::all(ink.with_alpha(*border_alpha*alpha)));
        assert_eq!(app.world().get::<BackgroundColor>(*entity).unwrap().0.alpha(),0.20*alpha);
        assert!(app.world().get::<ChildOf>(*entity).is_none(),"standalone overlays must be tested outside PipBoyRoot recolour ancestry");
    }
}
#[test]
fn actual_model_and_dismissal_palette_remains_pipboy_after_same_frame_core_recolour_and_graphics_mask() {
    let name=concat!(module_path!(),"::actual_model_and_dismissal_palette_remains_pipboy_after_same_frame_core_recolour_and_graphics_mask");
    if pip_palette_child(name.split_once("::").unwrap().1) {return;}
    let mut scene=optional_scene();install_control_layout(&mut scene.app,1.0);closing_idle(&mut scene,5);
    scene.app.insert_resource(nvf_play::settings::SettingsFile(None));
    scene.app.add_plugins(nvf_play::ui_colour::UiColourPlugin);
    closing_idle(&mut scene,2);
    assert_eq!(nvf_play::interface::hud_rgb(),nvf_play::interface::AMBER_RGB);
    assert_eq!(nvf_play::interface::pipboy_rgb(),nvf_play::interface::AMBER_RGB);
    // Real physical priority removes model controls. Keep the visual witness neutral;
    // hardware preservation is a separate final phase after all geometry checks.
    assert!(!scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    assert!(scene.app.world().resource::<crate::touch::TouchState>().physical_keys.is_empty());
    let mut targets=Vec::new();
    for (label,border_alpha) in [("Close",0.65),(nvf_play::pipboy::Page::Stats.title(),0.55)] {
        let (entity,rect,camera)=painted_control(&mut scene.app,label);
        let node=scene.app.world().get::<Node>(entity).unwrap().clone();
        let caption=pip_palette_caption(&mut scene.app,label);
        targets.push((entity,node,rect,camera,caption,border_alpha));
    }
    let geometry=scene.app.world().resource::<crate::touch::pipboy_model::Geometry>().clone();
    assert_eq!(geometry.buttons.len(),3,"the real posed model publisher must supply controls");
    let regions=scene.app.world().resource::<State>().regions.clone();assert_eq!(regions.len(),3);
    pip_palette_assert(&mut scene.app,&targets,true);
    let hud=scene.app.world().resource::<nvf_play::settings::Settings>().index("hud-colour").unwrap();
    assert!(nvf_play::settings::pick(scene.app.world_mut(),hud,"Blue"));
    scene.app.update(); // EXACT same-frame final paint: an extra settle would hide the old bug.
    assert_eq!(nvf_play::interface::pipboy_rgb(),nvf_play::interface::AMBER_RGB);
    assert_ne!(nvf_play::interface::hud_rgb(),nvf_play::interface::pipboy_rgb());
    pip_palette_assert(&mut scene.app,&targets,true);
    // Collinear changes expose accidental repeated brightness scaling as well.
    for rgb in [[200_u8,100,50],[100,50,25]] {
        pip_palette_rgb(&mut scene.app,false,rgb);pip_palette_rgb(&mut scene.app,true,rgb);
        scene.app.update();
        assert_eq!(nvf_play::interface::hud_rgb(),rgb);assert_eq!(nvf_play::interface::pipboy_rgb(),rgb);
        pip_palette_assert(&mut scene.app,&targets,true);
    }
    let graphics=scene.app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-button-graphics").unwrap();
    assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),graphics),Some(false));
    pip_palette_rgb(&mut scene.app,false,[50,25,12]);pip_palette_rgb(&mut scene.app,true,[25,50,100]);scene.app.update();
    pip_palette_assert(&mut scene.app,&targets,false);
    assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),graphics),Some(true));scene.app.update();
    pip_palette_assert(&mut scene.app,&targets,true);
    assert!(scene.app.world().resource::<crate::touch::pipboy_model::Geometry>()==&geometry);
    assert_eq!(scene.app.world().resource::<State>().regions,regions);
    assert!(scene.app.world().resource::<State>().drag.is_none());
    assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
    // Separate hardware phase: actual capture_physical keeps F12 authoritative,
    // and model.prepare must reject/draw must remove the touch model controls.
    scene.app.world_mut().write_message(bevy::input::keyboard::KeyboardInput{window:scene.window,key_code:KeyCode::F12,logical_key:bevy::input::keyboard::Key::F12,state:bevy::input::ButtonState::Pressed,text:None,repeat:false});
    scene.app.update();
    assert!(scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    assert!(scene.app.world().resource::<crate::touch::TouchState>().physical_keys.contains(&KeyCode::F12));
    {
        let world=scene.app.world_mut();let mut q=world.query::<&Text>();
        assert_eq!(q.iter(world).filter(|text|text.0==nvf_play::pipboy::Page::Stats.title()).count(),0,"physical priority removes actual model controls");
    }
    assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),graphics),Some(false));
    pip_palette_rgb(&mut scene.app,false,[80,40,20]);pip_palette_rgb(&mut scene.app,true,[40,80,160]);scene.app.update();
    assert!(scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    assert!(scene.app.world().resource::<crate::touch::TouchState>().physical_keys.contains(&KeyCode::F12));
    assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),graphics),Some(true));
    pip_palette_rgb(&mut scene.app,false,[100,50,25]);pip_palette_rgb(&mut scene.app,true,[50,100,200]);scene.app.update();
    assert!(scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12),"colour and graphics changes preserve the physical owner");
    assert!(scene.app.world().resource::<crate::touch::TouchState>().physical_keys.contains(&KeyCode::F12));
    assert!(scene.app.world().resource::<State>().drag.is_none());
    assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
    scene.app.world_mut().write_message(bevy::input::keyboard::KeyboardInput{window:scene.window,key_code:KeyCode::F12,logical_key:bevy::input::keyboard::Key::F12,state:bevy::input::ButtonState::Released,text:None,repeat:false});
    scene.app.update();
    assert!(!scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    assert!(scene.app.world().resource::<crate::touch::TouchState>().physical_keys.is_empty());
    let mut recovered=Vec::new();
    for (label,border_alpha) in [("Close",0.65),(nvf_play::pipboy::Page::Stats.title(),0.55)] {
        let (entity,rect,camera)=painted_control(&mut scene.app,label);
        let node=scene.app.world().get::<Node>(entity).unwrap().clone();
        let caption=pip_palette_caption(&mut scene.app,label);
        recovered.push((entity,node,rect,camera,caption,border_alpha));
    }
    assert_ne!(recovered[1].0,targets[1].0,"fresh model controls are recreated only after the real hardware release");
    for (fresh,original) in recovered.iter().zip(&targets) {
        assert_eq!((&fresh.1,fresh.2,fresh.3),(&original.1,original.2,original.3),"recovered controls retain actual measured geometry/camera");
    }
    assert_eq!(nvf_play::interface::pipboy_rgb(),[50,100,200]);
    pip_palette_assert(&mut scene.app,&recovered,true);
    assert!(scene.app.world().resource::<crate::touch::pipboy_model::Geometry>()==&geometry);
    assert_eq!(scene.app.world().resource::<State>().regions,regions);
}
