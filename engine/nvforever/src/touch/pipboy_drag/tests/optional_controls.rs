// These use the sole parent raw reader and real posed mesh publication. The real
// MenuPlugin closes fallback cases; no test handler removes the menu or changes NextState.
fn optional_scene() -> ClosingScene {
    let mut scene=closing_scene();
    scene.app.insert_resource(nvf_play::touch_safe_area::SafeInsets {left:18.,top:16.,right:22.,bottom:26.});
    *scene.app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>()=crate::touch::pipboy_options::Options {left:true,right:true,close:true,..default()};
    // New handles are independent of the old bottom-swipe enable switch.
    scene.app.world_mut().resource_mut::<crate::touch::drawer::Options>().enabled=false;
    closing_idle(&mut scene,2);scene
}
fn optional_point(scene:&ClosingScene,target:crate::touch::pipboy_options::Target)->Vec2 {
    scene.app.world().resource::<State>().regions.iter().find(|(t,_)|*t==target).expect("real painted footprint must leave this configured handle unclaimed").1.center()
}
// Side travel is now independently saved (default120), rather than the entire
// space below a cue. Calibrate the input, retaining every original effect,
// reversal, cancel, hardware, fresh-owner and native-close assertion.
fn optional_down(scene:&ClosingScene,p:Vec2,fraction:f32)->Vec2 {
    let safe=scene.app.world().resource::<nvf_play::touch_safe_area::SafeInsets>().bottom.max(closing_safe_bottom(scene));
    let travel=120.0_f32.min(closing_travel(closing_size(scene),safe,p).unwrap());
    let deadzone=8.0_f32.min(travel*0.25);
    p+Vec2::Y*(deadzone+(travel-deadzone)*fraction)
}

#[test]
fn real_parent_left_and_right_dismissal_reverses_the_same_arm_and_cancel_preserves_hardware() {
    for target in [crate::touch::pipboy_options::Target::Left,crate::touch::pipboy_options::Target::Right] {
        for graphics in [true,false] {
            let mut scene=optional_scene();scene.app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics=graphics;
            scene.app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {window:scene.window,key_code:KeyCode::F12,logical_key:bevy::input::keyboard::Key::F12,state:bevy::input::ButtonState::Pressed,text:None,repeat:false});
            scene.app.update();
            let p=optional_point(&scene,target);
            closing_edge(&mut scene,810,TouchPhase::Started,p);scene.app.update();
            assert_eq!(scene.app.world().resource::<ArmGesture>().progress(),Some(1.));
            for (fraction,want) in [(0.6,0.4),(0.2,0.8)] {
                let moved=optional_down(&scene,p,fraction);closing_edge(&mut scene,810,TouchPhase::Moved,moved);scene.app.update();
                assert!((scene.app.world().resource::<ArmGesture>().progress().unwrap()-want).abs()<1e-5);
            }
            let mut nodes=scene.app.world_mut().query_filtered::<Entity,With<OptionalVisual>>();
            let before=nodes.iter(scene.app.world()).collect::<Vec<_>>();
            // Paint-only change while held cannot cancel or move its target.
            scene.app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics = !graphics;
            scene.app.update();assert!((scene.app.world().resource::<ArmGesture>().progress().unwrap()-0.8).abs()<1e-5);
            let mut nodes=scene.app.world_mut().query_filtered::<Entity,With<OptionalVisual>>();
            assert_eq!(nodes.iter(scene.app.world()).collect::<Vec<_>>(),before);

            let reversed=optional_down(&scene,p,0.2);closing_edge(&mut scene,810,TouchPhase::Canceled,reversed);scene.app.update();
            assert!(scene.app.world().resource::<State>().drag.is_none());
            assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"cancel cannot issue native Close");
            assert!(scene.app.world().resource::<ArmGesture>().captures(),"actual core owns cancellation settlement");
            assert!(scene.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12),"virtual dismissal never releases the physical owner");
        }
    }
}
#[test]
fn actual_parent_dismissal_regions_yield_to_posed_content_and_retire_on_options_safe_area_and_controller_changes() {
    for cause in ["painted_screen","disable","insets","same_logical_dpi","controller"] {
        let mut scene=optional_scene();let p=optional_point(&scene,crate::touch::pipboy_options::Target::Left);
        if cause=="painted_screen" {
            let screen=scene.screen;closing_place(&mut scene,screen,p);closing_idle(&mut scene,2);
            assert!(!scene.app.world().resource::<State>().regions.iter().any(|(t,_)|*t==crate::touch::pipboy_options::Target::Left));
            closing_edge(&mut scene,820,TouchPhase::Started,p);scene.app.update();
            assert!(scene.app.world().resource::<State>().drag.is_none());continue;
        }
        closing_edge(&mut scene,820,TouchPhase::Started,p);scene.app.update();
        assert!(scene.app.world().resource::<State>().drag.is_some());
        match cause {
            "disable"=>scene.app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().left=false,
            "insets"=>scene.app.world_mut().resource_mut::<nvf_play::touch_safe_area::SafeInsets>().left+=5.,
            "same_logical_dpi"=> {let mut w=scene.app.world_mut().get_mut::<Window>(scene.window).unwrap();w.resolution=bevy::window::WindowResolution::new(2000,2000);w.resolution.set_scale_factor_override(Some(2.0));assert_eq!(w.resolution.size(),Vec2::splat(1000.));},
            _=>scene.app.world_mut().resource_mut::<nvf_play::bindings::Bindings>().device=nvf_play::bindings::InputDevice::Controller,
        }
        closing_edge(&mut scene,820,TouchPhase::Ended,p+Vec2::Y*300.);scene.app.update();
        assert!(scene.app.world().resource::<State>().drag.is_none());assert!(scene.app.world().resource::<State>().pending.is_none());
        assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
    }
}
pub(crate) fn install_native_close_consumer(app:&mut App) {
    // MenuPlugin has no Startup resource initializer. Install its exact public
    // systems over the existing authored SimState, including actual apply_input
    // and OnExit close_menu. No copied menu effects or private test-only helper.
    for key in ["NVF_PLAY_TEST_MENU","NVF_PLAY_TEST_BARTER","NVF_PLAY_TEST_DROP"] {assert!(std::env::var_os(key).is_none(),"run with test-aid inputs cleared");}
    app.init_resource::<nvf_play::pause::PlayClock>()
        .insert_resource(nvf_play::Location {world:0,interior:None})
        .init_resource::<nvf_play::pad::PadGlyphs>()
        .init_resource::<Assets<Image>>()
        .add_message::<nvf_play::OpenContainer>()
        .add_message::<nvf_play::pickpocket::OpenPickpocket>()
        .add_message::<nvf_play::RefChanged>()
        .add_message::<nvf_play::ItemUsed>()
        .add_message::<nvf_play::ui_sounds::UiSound>()
        .add_message::<nvf_play::crime::ItemsTaken>()
        .add_message::<nvf_play::pickpocket::Pickpocketed>()
        .add_plugins(nvf_play::menu::MenuPlugin);
    app.insert_resource(nvf_play::settings::SettingsFile(None));
    app.world_mut().resource_mut::<Options>().direct=false;

}
#[test]
fn real_parent_configured_close_and_down_swipes_reach_actual_native_menu_close_with_invisible_identical_targets() {
    for target in [crate::touch::pipboy_options::Target::Left,crate::touch::pipboy_options::Target::Right,crate::touch::pipboy_options::Target::Close] {
        for graphics in [true,false] {
            let mut scene=optional_scene();install_native_close_consumer(&mut scene.app);closing_idle(&mut scene,3);
            scene.app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics=graphics;
            let p=optional_point(&scene,target);
            closing_edge(&mut scene,830,TouchPhase::Started,p);scene.app.update();
            assert!(scene.app.world().resource::<State>().drag.is_some());
            assert!(!scene.app.world().resource::<ArmGesture>().captures(),"unavailable/disabled real arm is not fabricated");
            let end=if target==crate::touch::pipboy_options::Target::Close {p} else {optional_down(&scene,p,0.75)};
            closing_edge(&mut scene,830,TouchPhase::Moved,end);scene.app.update();
            assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"moving alone cannot close");
            closing_edge(&mut scene,830,TouchPhase::Ended,end);closing_idle(&mut scene,3);
            assert_eq!(*scene.app.world().resource::<bevy::prelude::State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Playing);
            assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"actual native OnExit retires the inventory");
            assert!(scene.app.world().resource::<State>().pending.is_none());
        }
    }
}
#[test]
fn fallback_dismissal_cancel_reverse_and_late_owner_change_do_not_close_or_replay() {
    for cause in ["cancel","reverse","late_controller","late_hidden_screen"] {
        let mut scene=optional_scene();install_native_close_consumer(&mut scene.app);closing_idle(&mut scene,3);
        let p=optional_point(&scene,crate::touch::pipboy_options::Target::Right);
        closing_edge(&mut scene,840,TouchPhase::Started,p);scene.app.update();
        let down=optional_down(&scene,p,0.8);closing_edge(&mut scene,840,TouchPhase::Moved,down);scene.app.update();
        let end=optional_down(&scene,p,if cause=="reverse" {0.2} else {0.8});
        if cause=="late_controller" {
            #[derive(Resource)]struct Once(bool);
            fn change(mut once:ResMut<Once>,mut b:ResMut<nvf_play::bindings::Bindings>) {if std::mem::take(&mut once.0) {b.device=nvf_play::bindings::InputDevice::Controller;}}
            scene.app.insert_resource(Once(true)).add_systems(Update,change.before(dispatch_close));
        }
        if cause=="late_hidden_screen" {
            #[derive(Resource)]struct Hide(Option<Entity>);
            fn hide(mut once:ResMut<Hide>,mut parts:Query<&mut Visibility>) {if let Some(entity)=once.0.take() {*parts.get_mut(entity).unwrap()=Visibility::Hidden;}}
            scene.app.insert_resource(Hide(Some(scene.screen))).add_systems(Update,hide.after(nvf_play::pipboy::PointerSet).before(dispatch_close));
        }
        closing_edge(&mut scene,840,if cause=="cancel" {TouchPhase::Canceled} else {TouchPhase::Ended},end);
        closing_idle(&mut scene,3);
        assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
        assert!(scene.app.world().resource::<State>().pending.is_none());
        scene.app.world_mut().resource_mut::<nvf_play::bindings::Bindings>().device=nvf_play::bindings::InputDevice::KeyboardMouse;
        scene.app.world_mut().entity_mut(scene.screen).insert(Visibility::Visible);
        closing_idle(&mut scene,3);assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"denied close never replays");
        closing_edge(&mut scene,841,TouchPhase::Started,p);scene.app.update();
        let fresh=optional_down(&scene,p,0.75);closing_edge(&mut scene,841,TouchPhase::Ended,fresh);closing_idle(&mut scene,3);
        assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"a fresh deliberate dismissal still works");
    }
}

#[test]
fn actual_parent_explicit_close_button_hands_zero_pose_to_existing_real_arm_settlement() {
    let mut scene=optional_scene();let p=optional_point(&scene,crate::touch::pipboy_options::Target::Close);
    closing_edge(&mut scene,845,TouchPhase::Started,p);scene.app.update();
    assert_eq!(scene.app.world().resource::<ArmGesture>().progress(),Some(1.));
    closing_edge(&mut scene,845,TouchPhase::Ended,p);scene.app.update();
    assert_eq!(scene.app.world().resource::<ArmGesture>().progress(),Some(0.));
    assert!(scene.app.world().resource::<ArmGesture>().captures(),"ordinary core settlement owns native close and camera restoration");
    assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"wrapper never removes a native menu itself");
    assert!(scene.app.world().resource::<State>().pending.is_none());
}

include!("optional_context_tests.rs");
