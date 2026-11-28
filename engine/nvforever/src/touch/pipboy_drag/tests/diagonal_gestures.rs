fn diagonal_raw(scene:&mut ClosingScene,id:u64,phase:TouchPhase,at:Vec2) {
    closing_edge(scene,id,phase,at);scene.app.update();
}
fn diagonal_arm_scene(view:UVec2,density:f32,scale:f32)->ClosingScene {
    let mut scene=closing_scene_at(view);
    let physical=(view.as_vec2()*density).as_uvec2();
    let mut resolution=bevy::window::WindowResolution::new(physical.x,physical.y);
    resolution.set_scale_factor_override(Some(density));
    scene.app.world_mut().get_mut::<Window>(scene.window).unwrap().resolution=resolution;
    scene.app.world_mut().get_mut::<Camera>(scene.camera).unwrap().computed.target_info=Some(bevy::camera::RenderTargetInfo {physical_size:physical,scale_factor:density});
    scene.app.insert_resource(nvf_play::touch_safe_area::SafeInsets {left:18.,top:16.,right:22.,bottom:26.});
    for key in ["touch-pipboy-dismiss-left","touch-pipboy-dismiss-right"] {
        let row=scene.app.world().resource::<nvf_play::settings::Settings>().index(key).unwrap();
        assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),row),Some(true));
    }
    install_control_layout(&mut scene.app,scale);closing_idle(&mut scene,6);scene
}
fn diagonal_arm_point(scene:&mut ClosingScene,target:crate::touch::pipboy_options::Target)->Vec2 {
    let rect=scene.app.world().resource::<State>().regions.iter().find(|(t,_)|*t==target).expect("actual posed model must leave an explicit handle free").1;
    let density=scene.app.world().get::<Window>(scene.window).unwrap().scale_factor();
    let world=scene.app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<OptionalVisual>>();
    assert!(q.iter(world).any(|(node,at)| {
        let r=Rect::from_center_size(at.translation/density,node.size()/density);
        r.min.abs_diff_eq(rect.min,0.5)&&r.max.abs_diff_eq(rect.max,0.5)
    }),"real Taffy bounds must equal raw handle geometry");
    rect.center()
}
#[test]
fn real_arm_both_diagonal_down_handles_preserve_reversal_cancel_masks_and_physical_owner_at_phone_scales() {
    for (view,density,scale) in [(UVec2::new(640,360),1.,0.75),(UVec2::new(792,363),2.,1.5)] {
        for target in [crate::touch::pipboy_options::Target::Left,crate::touch::pipboy_options::Target::Right] {
            let mut scene=diagonal_arm_scene(view,density,scale);
            let at=diagonal_arm_point(&mut scene,target);
            let safe=scene.app.world().resource::<nvf_play::touch_safe_area::SafeInsets>().bottom.max(closing_safe_bottom(&scene));
            let available=closing_travel(closing_size(&scene),safe,at).unwrap();
            let travel=120.0_f32.min(available);
            let deadzone=8.0_f32.min(travel*0.25);
            let direction=if target==crate::touch::pipboy_options::Target::Left {1.} else {-1.};
            // A real physical key may coexist; dismissal never releases its owner.
            scene.app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {window:scene.window,key_code:KeyCode::F12,logical_key:bevy::input::keyboard::Key::F12,state:bevy::input::ButtonState::Pressed,text:None,repeat:false});scene.app.update();
            diagonal_raw(&mut scene,1600,TouchPhase::Started,at);
            assert_eq!(scene.app.world().resource::<ArmGesture>().progress(),Some(1.));
            let down=at+Vec2::new(direction*55.,deadzone+(travel-deadzone)*0.8);
            assert!((down-at).x.abs()>32. && (down-at).y>(down-at).x.abs());
            diagonal_raw(&mut scene,1600,TouchPhase::Moved,down);
            assert!((scene.app.world().resource::<ArmGesture>().progress().unwrap()-0.2).abs()<1e-5);
            assert!(!scene.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(1600));
            diagonal_raw(&mut scene,1600,TouchPhase::Moved,at+Vec2::new(direction*10.,deadzone+(travel-deadzone)*0.2));
            assert!((scene.app.world().resource::<ArmGesture>().progress().unwrap()-0.8).abs()<1e-5);
            diagonal_raw(&mut scene,1600,TouchPhase::Canceled,at+Vec2::new(direction*10.,deadzone+(travel-deadzone)*0.2));
            assert!(scene.app.world().resource::<State>().drag.is_none());
            assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
            assert!(scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
            assert!(scene.app.world().resource::<ArmGesture>().captures(),"actual core retains settlement authority; this fixture does not fake clip/close");
        }
    }
}
#[test]
fn real_parent_wide_safe_opening_band_accepts_diagonal_outside_old_handle_and_keeps_system_edge_free() {
    for canceled in [false,true] {
        let (mut app,window)=scene();install_control_layout(&mut app,1.5);
        for _ in 0..5 {app.update();}
        let size=app.world().get::<Window>(window).unwrap().resolution.size();
        let painted={let world=app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<super::super::drawer::Handle>>();let (node,at)=q.single(world).unwrap();Rect::from_center_size(at.translation,node.size())};
        let at=(16..(size.x as u32-16)).step_by(8).map(|x|Vec2::new(x as f32,painted.center().y)).find(|p|painted.contains(*p) && (p.x-size.x/2.).abs()>80. && app.world().resource::<crate::touch::Layout>().config.hit_filtered(*p,size,true,false,|_|true).is_none()).expect("painted wide band must expose a free start outside the old centered 144-unit handle");
        assert!(painted.height()>=47.5);
        assert!(at.y<size.y-24.);
        app.world_mut().write_message(TouchInput {window,id:1610,phase:TouchPhase::Started,position:at,force:None});app.update();
        assert_eq!(app.world().resource::<ArmGesture>().progress(),Some(0.));
        let end=at+Vec2::new(if at.x<size.x/2. {55.} else {-55.},-96.);
        app.world_mut().write_message(TouchInput {window,id:1610,phase:TouchPhase::Moved,position:end,force:None});app.update();
        assert_eq!(app.world().resource::<ArmGesture>().progress(),Some(1.));
        app.world_mut().write_message(TouchInput {window,id:1610,phase:if canceled {TouchPhase::Canceled}else{TouchPhase::Ended},position:end,force:None});app.update();
        assert!(app.world().resource::<State>().drag.is_none());
        assert!(app.world().resource::<ArmGesture>().captures());
        assert!(app.world().resource::<crate::touch::TouchState>().contacts.is_empty());
        assert!(app.world().resource::<bevy::ecs::message::Messages<nvf_play::menu::OpenInventory>>().is_empty(),"only the real core settle consumer may request the native endpoint");
    }
}

fn outside_model_display(scene:&mut ClosingScene,left:bool)->Vec2 {
    let geometry=scene.app.world().resource::<crate::touch::pipboy_model::Geometry>();
    let display=geometry.screen.expect("actual posed SCREEN publication");
    let safe=*scene.app.world().resource::<nvf_play::touch_safe_area::SafeInsets>();
    let size=closing_size(scene);
    let x=if left {(safe.left+display.min.x)*0.5} else {(display.max.x+size.x-safe.right)*0.5};
    let cue_regions=&scene.app.world().resource::<State>().regions;
    (0..8).map(|row|Vec2::new(x,safe.top+20.+row as f32*16.)).find(|p| {
        p.y<size.y-safe.bottom-80. && !display.contains(*p)
            && !geometry.buttons.iter().any(|b|b.bounds.contains(*p))
            && !geometry.knob.is_some_and(|r|r.contains(*p))
            && !cue_regions.iter().any(|(_,r)|r.contains(*p))
    }).expect("outside-display wrist/world space must offer a start beyond the tiny cues")
}
#[test]
fn actual_posed_screen_allows_both_outside_sides_continuous_reversal_beyond_edge_cues() {
    for left in [true,false] {for graphics in [true,false] {
        let mut scene=diagonal_arm_scene(UVec2::new(640,360),2.,1.5);
        if !graphics {let row=scene.app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-button-graphics").unwrap();assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),row),Some(false));closing_idle(&mut scene,4);}
        let at=outside_model_display(&mut scene,left);let size=closing_size(&scene);
        let travel=120.0_f32.min(size.y-26.-at.y);
        let deadzone=8.0_f32.min(travel*0.25);
        diagonal_raw(&mut scene,1620,TouchPhase::Started,at);
        assert_eq!(scene.app.world().resource::<ArmGesture>().progress(),Some(1.));
        for amount in [0.8,0.1,0.] {
            let down=if amount==0. {0.} else {deadzone+(travel-deadzone)*amount};
            diagonal_raw(&mut scene,1620,TouchPhase::Moved,at+Vec2::new(if left {40.} else {-40.},down));
            assert!((scene.app.world().resource::<ArmGesture>().progress().unwrap()-(1.-amount)).abs()<1e-5,"reversing a held finger must raise continuously, not jump or release");
            assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
        }
        diagonal_raw(&mut scene,1620,TouchPhase::Canceled,at);
        assert!(scene.app.world().resource::<State>().drag.is_none());
        assert!(scene.app.world().resource::<ArmGesture>().captures(),"normal core settlement still owns the pose; clip/body coupling has its own real-provider fixture");
    }}
}
#[test]
fn actual_arm_camera_frame_and_saved_side_changes_retire_the_same_contact_without_stale_end() {
    for cause in ["camera","frame","side","travel"] {
        let mut scene=diagonal_arm_scene(UVec2::new(640,360),1.,1.);
        let at=outside_model_display(&mut scene,true);
        diagonal_raw(&mut scene,1630,TouchPhase::Started,at);
        diagonal_raw(&mut scene,1630,TouchPhase::Moved,at+Vec2::new(16.,64.));
        let before=scene.app.world().resource::<ArmGesture>().progress();assert!(before.unwrap()<1.);
        match cause {
            "camera"=>{use bevy::camera::CameraProjection;scene.app.world_mut().get_mut::<Camera>(scene.camera).unwrap().computed.clip_from_view=PerspectiveProjection {fov:0.9,aspect_ratio:640./360.,..default()}.get_clip_from_view();},
            "frame"=>scene.app.world_mut().resource_mut::<PipBoy>().frame=Frame::Screen,
            "side"=>{let row=scene.app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-dismiss-side-v2").unwrap();assert!(nvf_play::settings::pick(scene.app.world_mut(),row,"Disabled"));},
            "travel"=>{let row=scene.app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-dismiss-travel-v1").unwrap();nvf_play::settings::set_slider(scene.app.world_mut(),row,1.);},
            _=>unreachable!(),
        }
        diagonal_raw(&mut scene,1630,TouchPhase::Ended,at+Vec2::new(20.,120.));
        assert!(scene.app.world().resource::<State>().drag.is_none());
        assert_eq!(scene.app.world().resource::<ArmGesture>().progress(),before,"retired End cannot advance the old pose");
        assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
        assert!(!scene.app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(1630));
    }
}

#[test]
fn actual_arm_opening_keeps_vertical_winner_when_reversing_with_persistent_side_drift() {
    for horizontal_first in [false,true] {
        let (mut app,window)=scene();
        app.init_resource::<nvf_play::menu::ActiveMenu>();
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
        install_control_layout(&mut app,1.);
        for _ in 0..5 {app.update();}
        let size=app.world().get::<Window>(window).unwrap().resolution.size();
        let handle_rect={let world=app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<super::super::drawer::Handle>>();let (node,at)=q.single(world).unwrap();assert!(node.size().min_element()>0.);Rect::from_center_size(at.translation,node.size())};
        let handle=(16..(size.x as u32-56)).step_by(8).map(|x|Vec2::new(x as f32,handle_rect.center().y)).find(|p|handle_rect.contains(*p) && app.world().resource::<crate::touch::Layout>().config.hit_filtered(*p,size,true,false,|_|true).is_none()).expect("actual bottom band must have a layout-unclaimed start");
        let edge=|phase,position|TouchInput {window,id:1645,phase,position,force:None};
        app.world_mut().write_message(edge(TouchPhase::Started,handle));app.update();
        assert_eq!(app.world().resource::<ArmGesture>().progress(),Some(0.));
        if horizontal_first {app.world_mut().write_message(edge(TouchPhase::Moved,handle+Vec2::X*40.));app.update();}
        for dy in [-80.,-20.,-96.] {
            app.world_mut().write_message(edge(TouchPhase::Moved,handle+Vec2::new(40.,dy)));app.update();
            let expected=if horizontal_first {0.} else {(-dy/96.0_f32).clamp(0.,1.)};
            assert!((app.world().resource::<ArmGesture>().progress().unwrap()-expected).abs()<1e-5);
            assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
        }
        app.world_mut().write_message(edge(TouchPhase::Canceled,handle+Vec2::new(40.,-96.)));app.update();
        assert!(app.world().resource::<State>().drag.is_none());
        assert!(app.world().resource::<ArmGesture>().captures());
        assert_eq!(size,app.world().get::<Window>(window).unwrap().resolution.size());
    }
}
