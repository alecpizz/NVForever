// Real raw keyboard/gamepad producers, physical provenance and the production
// touch angle -> keyboard angle -> lockpick apply chain. No render/device claim.
#[derive(Resource,Default)]
struct InputUnionSource { logical_right: bool, physical_right: bool }
fn input_union_source(input:ControlInput,mut seen:ResMut<InputUnionSource>) {
    seen.logical_right=input.pressed(Control::SlideRight);
    seen.physical_right=input.physical_pressed(Control::SlideRight);
}
fn input_union_scene(moving:bool)->(App,Entity,Entity) {
    let mut app=App::new();
    app.add_plugins((MinimalPlugins,bevy::input::InputPlugin,crate::pad::InputSwitchPlugin))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
        .init_resource::<Bindings>().init_resource::<crate::pad::StickButtons>()
        .init_resource::<crate::pause::PlayClock>().init_resource::<LockpickStats>()
        .init_resource::<InputUnionSource>()
        .insert_resource(crate::sim::test_world::sim(Vec::new(),Vec::new()))
        .add_message::<LockpickInput>().add_message::<UiSound>().add_message::<LockOpened>()
        .add_systems(PreUpdate,crate::pad::update_stick_buttons.after(crate::pad::InputSwitchSystems::Switch));
    let mut script=ScriptState::over_no_plugins();
    script.rt.as_mut().unwrap().game.tweaks.lockpick_move_keys=moving;
    app.insert_resource(script);
    touch::install(&mut app);
    app.world_mut().resource_mut::<Bindings>().device=crate::bindings::InputDevice::Controller;
    app.world_mut().resource_mut::<Bindings>().automatic_input_switching=false;
    app.insert_resource(LockpickScreen(Some(Session {
        reference:FormId(7),practice:true,name:"Authored input lock".into(),
        game:Picking::new(rules::Level::VeryEasy,100.0,2,0.5,Settings::default()),
        status:None,opened:1.0,pin_clip:PinClip::In,pin_since:1.0,ended:None,help:None,
        words:Words {skill:"Skill".into(),picks:"Pins".into(),level:"Level".into(),exit:"Leave".into(),force_hint:"F".into(),exit_hint:"E".into()},
        leaving:None,moved:0.0,aid_torque_until:None,
    })));
    app.add_systems(Update,(input_union_source,touch::input,keys,apply).chain());
    let window=app.world_mut().spawn((Window {resolution:(960,540).into(),focused:true,..default()},bevy::window::PrimaryWindow)).id();
    let pad=app.world_mut().spawn(Gamepad::default()).id();
    for _ in 0..3 {app.update();} // real controller neutral-on-entry
    (app,window,pad)
}
fn input_union_key(app:&mut App,window:Entity,code:KeyCode,down:bool) {
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
        window,key_code:code,logical_key:bevy::input::keyboard::Key::Character("authored".into()),
        state:if down {bevy::input::ButtonState::Pressed}else{bevy::input::ButtonState::Released},text:None,repeat:false,
    });app.update();
}
fn input_union_pin(app:&App)->f32 {app.world().resource::<LockpickScreen>().0.as_ref().unwrap().game.pin}
#[test]
fn keyboard_movement_uses_only_real_keyboard_while_pad_angle_has_one_owner() {
    // keys_with_pad is process-global; use exactly this shared test binary alone.
    const CHILD:&str="NVF_TEST_LOCKPICK_INPUT_UNION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name=concat!(module_path!(),"::keyboard_movement_uses_only_real_keyboard_while_pad_angle_has_one_owner");
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact",name.split_once("::").unwrap().1,"--test-threads=1"]).env(CHILD,"1").spawn().unwrap();
        let start=std::time::Instant::now();
        let status=loop {match child.try_wait() {
            Ok(Some(status))=>break status,
            Ok(None) if start.elapsed()<std::time::Duration::from_secs(30)=>std::thread::sleep(std::time::Duration::from_millis(10)),
            Ok(None)=>{let killed=child.kill();let reaped=child.wait();panic!("input union child exceeded30s: kill={killed:?}, reap={reaped:?}");}
            Err(error)=>{let killed=child.kill();let reaped=child.wait();panic!("input union child wait {error}: kill={killed:?}, reap={reaped:?}");}
        }};assert!(status.success());return;
    }
    use bevy::input::gamepad::{RawGamepadEvent,RawGamepadAxisChangedEvent};
    crate::bindings::set_keys_with_pad(false);
    let mut pad_angles=Vec::new();
    for moving in [false,true] {
        let (mut app,_,pad)=input_union_scene(moving);let before=input_union_pin(&app);
        app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad,GamepadAxis::LeftStickX,0.8)));
        app.update();
        let source=app.world().resource::<InputUnionSource>();
        assert!(source.logical_right,"the old generic control path really sees the pad angle");
        assert!(!source.physical_right,"raw pad axes are never physical keyboard");
        assert!(app.world().get::<Gamepad>(pad).unwrap().left_stick().x>0.7);
        assert!(input_union_pin(&app)>before);
        pad_angles.push(input_union_pin(&app));
    }
    assert!((pad_angles[0]-pad_angles[1]).abs()<1e-5,"keyboard-only tweak cannot duplicate the production pad angle");
    let (mut app,window,_)=input_union_scene(true);let before=input_union_pin(&app);
    input_union_key(&mut app,window,KeyCode::KeyD,true);
    assert_eq!(input_union_pin(&app),before,"selected controller keeps keyboard opt-in off");
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyD));
    input_union_key(&mut app,window,KeyCode::KeyD,false);
    crate::bindings::set_keys_with_pad(true);
    input_union_key(&mut app,window,KeyCode::KeyD,true);
    assert!(app.world().resource::<InputUnionSource>().physical_right);
    assert!(input_union_pin(&app)>before,"explicit controller keyboard opt-in uses actual raw key");
    assert_eq!(app.world().resource::<Bindings>().device,crate::bindings::InputDevice::Controller);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyD));
    input_union_key(&mut app,window,KeyCode::KeyD,false);
    // A virtual aggregate key has no KeyboardInput event or raw source ownership.
    #[derive(Resource)]struct Virtual(Option<bool>);
    fn virtual_key(mut on:ResMut<Virtual>,mut keys:ResMut<ButtonInput<KeyCode>>) {if let Some(on)=on.0.take() {if on {keys.press(KeyCode::KeyD);}else{keys.release(KeyCode::KeyD);}}}
    app.insert_resource(Virtual(Some(true))).add_systems(PreUpdate,virtual_key.after(crate::pad::InputSwitchSystems::Switch));
    let before=input_union_pin(&app);app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyD));
    assert!(!app.world().resource::<InputUnionSource>().physical_right);
    assert_eq!(input_union_pin(&app),before,"virtual aggregate source cannot become keyboard-only angle input");
    app.world_mut().resource_mut::<Virtual>().0=Some(false);app.update();
    crate::bindings::set_keys_with_pad(false);
    app.world_mut().resource_mut::<Bindings>().device=crate::bindings::InputDevice::KeyboardMouse;
    let before=input_union_pin(&app);input_union_key(&mut app,window,KeyCode::KeyD,true);
    assert!(input_union_pin(&app)>before,"ordinary keyboard movement remains available");
    input_union_key(&mut app,window,KeyCode::KeyD,false);
    app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.tweaks.lockpick_move_keys=false;
    let before=input_union_pin(&app);input_union_key(&mut app,window,KeyCode::ArrowRight,true);
    assert!(input_union_pin(&app)>before,"existing arrow-key fallback remains independent");
    input_union_key(&mut app,window,KeyCode::ArrowRight,false);
}
