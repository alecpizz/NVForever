// Actual InputPlugin/Bindings/player::mouse_look consumers at the Main/Android seam.
#[test]
fn main_aim_sensitivity_scales_actual_right_stick_by_zoom_share() {
    for (blend, factor) in [(0.0,1.0),(0.5,0.75),(1.0,0.5)] {
        let sample = |gain:f32| {
            let (mut app,pad,_)=scene();
            crate::bindings::set_aim_sensitivity(app.world_mut(),gain);
            app.world_mut().resource_mut::<crate::aim_zoom::AimZoom>().blend=blend;
            axis(&mut app,pad,GamepadAxis::RightStickX,0.6);
            let before=app.world().resource::<Player>().heading;app.update();
            let after=app.world().resource::<Player>().heading;
            assert_eq!(app.world().resource::<Bindings>().device,InputDevice::Controller);
            assert!(app.world().resource::<Sticks>().1.x>0.0);
            after-before
        };
        let normal=sample(1.0);let scaled=sample(0.5);
        assert!(normal>0.0,"actual raw right stick turned the player");
        assert!((scaled-normal*factor).abs()<1e-6,"Main device gain follows actual zoom share at {blend}");
    }
}
#[test]
fn main_aim_device_gain_and_inversion_do_not_rescale_independent_gyro_radians() {
    for (blend,factor) in [(0.0,1.0),(0.5,0.75),(1.0,0.5)] {
        let (mut app,_,_)=scene();
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching=false;
        crate::bindings::set_device(app.world_mut(),InputDevice::KeyboardMouse);
        crate::bindings::set_sensitivity(app.world_mut(),1.0);
        crate::bindings::set_vertical(app.world_mut(),1.0);
        crate::bindings::set_invert_y(app.world_mut(),true);
        crate::bindings::set_aim_sensitivity(app.world_mut(),0.5);
        for _ in 0..3 {app.update();}
        let device_gain=app.world().resource::<Bindings>().mouse_sensitivity;
        app.world_mut().resource_mut::<crate::aim_zoom::AimZoom>().blend=blend;
        let sensor=Vec2::new(0.03,0.04);
        app.insert_resource(crate::player::GyroLook {radians:sensor});
        let raw=Vec2::new(100.0,80.0);
        let before=(app.world().resource::<Player>().heading,app.world().resource::<Player>().pitch);
        app.world_mut().write_message(bevy::input::mouse::MouseMotion {delta:raw});app.update();
        let player=app.world().resource::<Player>();
        let expected=Vec2::new(raw.x,-raw.y)*device_gain*factor+sensor;
        assert!((player.heading-before.0-expected.x).abs()<1e-6,"only device counts take Main Aim Sensitivity");
        assert!((player.pitch-before.1+expected.y).abs()<1e-6,"device inversion precedes independently signed gyro");
        assert_eq!(app.world().resource::<crate::player::GyroLook>().radians,sensor);
        assert_eq!(app.world().resource::<Bindings>().device,InputDevice::KeyboardMouse);
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }
}
