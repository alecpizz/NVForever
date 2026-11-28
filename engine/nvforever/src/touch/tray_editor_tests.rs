mod tray_editor_regressions {
    use super::*;
    use nvf_play::{settings,touch_safe_area::SafeInsets};
    struct Files(std::path::PathBuf);
    impl Files {
        fn new()->Self {
            let path=std::env::temp_dir().join(format!("nvf-tray-editor-{}",uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();Self(path)
        }
        fn layout(&self)->std::path::PathBuf {self.0.join("touch-controls.json")}
    }
    impl Drop for Files {
        fn drop(&mut self) {
            for name in ["touch-controls.json","touch-controls.json.tmp","settings.ini"] {let _=std::fs::remove_file(self.0.join(name));}
            let _=std::fs::remove_dir(&self.0);
        }
    }
    fn request(app:&mut App) {
        let row=app.world().resource::<settings::Settings>().index(tray_editor::KEY).unwrap();
        assert!(settings::pick(app.world_mut(),row,"Edit tray"));app.update();app.update();
        assert!(app.world().resource::<Layout>().tray_editing());
        assert!(app.world().resource::<Layout>().tray_preview.is_some(),"actual fitter must supply preview geometry");
    }
    fn send(app:&mut App,w:Entity,id:u64,phase:TouchPhase,p:Vec2) {touch(app,w,id,phase,p.x,p.y);}
    fn action(app:&mut App,w:Entity,id:u64,index:usize) {
        let window=app.world().get::<Window>(w).unwrap();let size=Vec2::new(window.width(),window.height());
        let safe=app.world().resource::<Layout>().tray_safe;
        let p=tray_editor::action_rect_with(index,size,safe).center();
        send(app,w,id,TouchPhase::Started,p);send(app,w,id,TouchPhase::Ended,p);app.update();
    }
    fn drag(app:&mut App,w:Entity,id:u64,to:Vec2) {
        let p=app.world().resource::<Layout>().tray_preview.as_ref().unwrap().bounds.center();
        send(app,w,id,TouchPhase::Started,p);app.update();send(app,w,id,TouchPhase::Moved,to);app.update();
    }
    fn no_virtual_action(app:&App) {
        let state=app.world().resource::<TouchState>();
        assert!(state.contacts.is_empty() && state.keys.is_empty() && state.mouse.is_empty() && state.pulses.is_empty());
        assert!(!state.aim_latched && !app.world().resource::<nvf_play::control::HeldSneakInput>().active);
        assert_eq!(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>().len(),0);
        assert!(!app.world().resource::<gyro::State>().active(app.world().resource::<gyro::Options>()));
    }
    #[test]
    fn actual_tray_editor_cancel_restores_whole_profile_and_keeps_hardware_owners() {
        let files=Files::new();let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(792.,363.),"classic",4);
        {
            let mut layout=app.world_mut().resource_mut::<Layout>();layout.path=Some(files.layout());
            layout.config.extra["foreign"]=serde_json::json!({"keep":[1,2,3]});layout.config.store_slot(0).unwrap();
            layout.config.save_to(&files.layout()).unwrap();
        }
        let before=app.world().resource::<Layout>().config.clone();let bytes=std::fs::read(files.layout()).unwrap();
        let toggle=app.world().resource::<settings::Settings>().index("touch-hotbar").unwrap();
        assert_eq!(settings::flip(app.world_mut(),toggle),Some(false));
        app.insert_resource(nvf_play::pause::Paused(true));app.update();
        assert!(mixed_hotbar_tiles(&mut app).is_empty(),"ordinary live tray is hidden when disabled");
        raw_key(&mut app,w,KeyCode::F12,ButtonState::Pressed);
        app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {window:w,button:MouseButton::Right,state:ButtonState::Pressed});app.update();
        request(&mut app);
        assert!(!mixed_hotbar_tiles(&mut app).is_empty(),"action-free editor preview remains available while paused and disabled");
        drag(&mut app,w,930,Vec2::new(430.,140.));
        let anchor=app.world().resource::<Layout>().config.tray.center;
        let second=app.world().resource::<Layout>().tray_preview.as_ref().unwrap().bounds.center();
        send(&mut app,w,939,TouchPhase::Started,second);send(&mut app,w,939,TouchPhase::Moved,Vec2::new(700.,100.));app.update();
        assert_eq!(app.world().resource::<Layout>().config.tray.center,anchor,"second finger cannot steal the editor drag owner");
        action(&mut app,w,931,1);
        assert_ne!(app.world().resource::<Layout>().config,before);
        assert_eq!(std::fs::read(files.layout()).unwrap(),bytes,"preview never writes the settings file");
        no_virtual_action(&app);
        action(&mut app,w,932,3);
        assert!(app.world().resource::<Layout>().editor.is_none());
        assert_eq!(app.world().resource::<Layout>().config,before);
        assert_eq!(std::fs::read(files.layout()).unwrap(),bytes);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Right));
        no_virtual_action(&app);
    }
    #[test]
    fn actual_preview_save_persists_geometry_and_drops_background_touch_tail() {
        let files=Files::new();let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(792.,363.),"classic",4);
        app.world_mut().resource_mut::<Layout>().path=Some(files.layout());
        request(&mut app);drag(&mut app,w,940,Vec2::new(400.,170.));action(&mut app,w,941,1);
        let wanted=app.world().resource::<Layout>().config.clone();
        let save=tray_editor::action_rect(2,Vec2::new(792.,363.)).center();
        let fire=app.world().resource::<Layout>().config.center(Target::Button(7),Vec2::new(792.,363.));
        let aim=app.world().resource::<Layout>().config.center(Target::Button(6),Vec2::new(792.,363.));
        send(&mut app,w,942,TouchPhase::Started,save);
        for (id,p) in [(943,fire),(944,aim)] {send(&mut app,w,id,TouchPhase::Started,p);send(&mut app,w,id,TouchPhase::Ended,p);}
        app.update();no_virtual_action(&app);
        assert!(app.world().resource::<Layout>().editor.is_none());
        let loaded=layout::Config::decode(&serde_json::from_slice(&std::fs::read(files.layout()).unwrap()).unwrap()).unwrap();
        assert_eq!(loaded,wanted);assert!(loaded.tray_explicit);
        app.update();no_virtual_action(&app);
        assert!(!files.layout().with_extension("json.tmp").exists());
    }
    #[test]
    fn actual_editor_slots_and_large_toolbar_share_safe_fitted_render_bounds() {
        for (size,count) in [(Vec2::new(792.,363.),4),(Vec2::new(1280.,720.),8),(Vec2::new(800.,1000.),4)] {
            let (mut app,_w)=mixed_hotbar_phone_scene(size,"explorer",count);
            let safe=SafeInsets {left:20.,top:12.,right:16.,bottom:24.};app.insert_resource(safe);app.update();app.update();request(&mut app);
            let preview=app.world().resource::<Layout>().tray_preview.as_ref().unwrap().clone();
            let painted=mixed_hotbar_tiles(&mut app);
            assert_eq!(preview.slots.len(),painted.len());
            let usable=Rect::from_corners(Vec2::new(safe.left,safe.top),size-Vec2::new(safe.right,safe.bottom+64.));
            for rect in &preview.slots {
                assert!(rect.width()>=48. && rect.height()>=48.);
                assert!(usable.contains(rect.min) && usable.contains(rect.max),"{size:?}: all fitted slots avoid system insets and navigation band");
                assert!(painted.iter().any(|(_,shown)|shown==rect),"preview and actual drawn slot must be identical");
                for i in 0..4 {let overlap=rect.intersect(tray_editor::action_rect_with(i,size,safe));assert!(overlap.width()<=0. || overlap.height()<=0.);}
            }
            let world=app.world_mut();let mut q=world.query::<(&TouchVisual,&Node,&Visibility)>();
            let actions=q.iter(world).filter_map(|(v,n,visibility)|match v {TouchVisual::EditorAction(i) if *visibility==Visibility::Inherited=>Some((*i,n)),_=>None}).collect::<Vec<_>>();
            assert_eq!(actions.len(),4,"dedicated view shows only Smaller/Larger/Save/Cancel");
            for (i,node) in actions {let index=[0,1,9,10].iter().position(|v|*v==i).unwrap();let r=tray_editor::action_rect_with(index,size,safe);assert_eq!((node.left,node.top,node.width,node.height),(px(r.min.x),px(r.min.y),px(r.width()),px(r.height())));assert!(r.width()>=48. && r.height()>=48.);}
        }
    }
    #[test]
    fn actual_resize_focus_and_platform_interruptions_retire_editor_drag_until_new_start() {
        for scenario in 0..4 {
            let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(792.,363.),"classic",4);request(&mut app);
            drag(&mut app,w,950,Vec2::new(400.,150.));let center=app.world().resource::<Layout>().config.tray.center;
            match scenario {
                0=>app.world_mut().get_mut::<Window>(w).unwrap().resolution.set(900.,420.),
                1=>{app.world_mut().write_message(WindowFocused {window:w,focused:false});},
                2=>{app.init_resource::<nvf_play::pad::PlatformInputGate>();app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);},
                _=>{app.insert_resource(nvf_play::dialogue_touch::TouchDialogue {enabled:true,..default()});app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Dialogue);},
            }
            app.update();send(&mut app,w,950,TouchPhase::Moved,Vec2::new(100.,100.));app.update();
            assert_eq!(app.world().resource::<Layout>().config.tray.center,center,"interrupted owner cannot keep moving preview");
            if scenario==1 {app.world_mut().write_message(WindowFocused {window:w,focused:true});}
            if scenario==2 {app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(false);}
            if scenario==3 {
                assert!(app.world().resource::<Layout>().tray_preview.is_none(),"dedicated modal hides tray preview in the same input frame");
                assert!(mixed_hotbar_tiles(&mut app).is_empty());
                app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Playing);
            }
            app.update();app.update();app.update();
            send(&mut app,w,950,TouchPhase::Ended,Vec2::new(100.,100.));app.update();
            assert_eq!(app.world().resource::<Layout>().config.tray.center,center);
            drag(&mut app,w,951,Vec2::new(600.,180.));assert_ne!(app.world().resource::<Layout>().config.tray.center,center);
            no_virtual_action(&app);
        }
    }
    #[test]
    fn profile_tray_roundtrip_legacy_slot_merge_and_future_version_refusal_are_lossless() {
        let files=Files::new();let mut c=layout::Config::default();
        c.extra["foreign"]=serde_json::json!({"untouched":true});c.extra["tray"]=serde_json::json!({"future_field":[7,8]});
        c.tray.center=Vec2::new(0.3,0.4);c.tray.scale=0.6;c.tray_explicit=true;c.store_slot(0).unwrap();let first=c.tray.clone();
        c.tray.center=Vec2::new(0.7,0.5);c.tray.scale=1.8;c.store_slot(1).unwrap();let second=c.tray.clone();
        c.load_slot(0).unwrap();assert_eq!(c.tray,first);c.load_slot(1).unwrap();assert_eq!(c.tray,second);
        c.extra["profiles"][0]["layout"].as_object_mut().unwrap().remove("tray");c.load_slot(0).unwrap();assert_eq!(c.tray,second,"older slot must not reset newer tray placement");
        c.save_to(&files.layout()).unwrap();let loaded=layout::Config::decode(&serde_json::from_slice(&std::fs::read(files.layout()).unwrap()).unwrap()).unwrap();
        assert_eq!(loaded,c);assert_eq!(loaded.encode()["tray"]["future_field"],serde_json::json!([7,8]));
        let mut future=c.encode();future["tray"]=serde_json::json!({"version":99,"opaque":{"keep":[1,2]}});std::fs::write(files.layout(),serde_json::to_vec(&future).unwrap()).unwrap();
        let mut future_config=layout::Config::decode(&future).unwrap();let original=future_config.encode()["tray"].clone();
        let mut layout=Layout {config:future_config.clone(),..default()};assert!(layout.open_tray().is_err());
        future_config.hints = !future_config.hints;future_config.save_to(&files.layout()).unwrap();assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(files.layout()).unwrap()).unwrap()["tray"],original);
        layout.open(true);
        let reset=layout::editor_rect(8,Vec2::new(792.,363.)).center();
        layout.event(&TouchInput {window:Entity::PLACEHOLDER,id:1,phase:TouchPhase::Started,position:reset,force:None},Vec2::new(792.,363.));
        assert_eq!(layout.config.encode()["tray"],original,"ordinary control reset cannot downgrade an unsupported tray");
        assert!(!layout.config.tray_supported);
        let before=std::fs::read(files.layout()).unwrap();assert!(c.save_to(&files.layout()).is_err());assert_eq!(std::fs::read(files.layout()).unwrap(),before);
        c.extra["profiles"][0]["layout"]["tray"]=original;let before=c.encode();assert!(c.store_slot(0).is_err());assert_eq!(c.encode(),before);
    }

    #[test]
    fn persistent_controller_and_suspend_hide_preview_and_retire_drag_until_fresh_start() {
        use nvf_play::bindings::{set_device,InputDevice};
        for suspended in [false,true] {
            let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(792.,363.),"classic",4);request(&mut app);
            drag(&mut app,w,980,Vec2::new(400.,150.));let before=app.world().resource::<Layout>().config.tray.center;
            if suspended {app.world_mut().write_message(AppLifecycle::Suspended);} else {set_device(app.world_mut(),InputDevice::Controller);}
            // No repeated lifecycle event: the persistent state must keep blocking.
            for _ in 0..4 {
                app.update();assert!(app.world().resource::<Layout>().tray_preview.is_none());assert!(mixed_hotbar_tiles(&mut app).is_empty());
                assert!(!app.world().resource::<nvf_play::pipboy::touch_items::OverlayCapture>().0);
                assert_eq!(hotbar::test_occlusion(app.world()),(false,vec![]),"hidden preview has no stale capturing flag or fitted exclusion rectangles");
                assert!(!app.world().resource::<TouchState>().ui_capture,"this controller/suspend fixture has no other UI owner");
            }
            send(&mut app,w,980,TouchPhase::Moved,Vec2::new(100.,100.));app.update();
            assert_eq!(app.world().resource::<Layout>().config.tray.center,before);
            if suspended {app.world_mut().write_message(AppLifecycle::Running);} else {set_device(app.world_mut(),InputDevice::KeyboardMouse);}
            app.update();app.update();app.update();
            assert!(app.world().resource::<Layout>().tray_preview.is_some());
            send(&mut app,w,980,TouchPhase::Moved,Vec2::new(200.,100.));app.update();
            assert_eq!(app.world().resource::<Layout>().config.tray.center,before,"old held contact cannot rearm its draft drag after device/lifecycle return");
            send(&mut app,w,980,TouchPhase::Ended,Vec2::new(200.,100.));app.update();
            drag(&mut app,w,981,Vec2::new(600.,170.));assert_ne!(app.world().resource::<Layout>().config.tray.center,before);
            assert!(!app.world().resource::<nvf_play::pipboy::touch_items::OverlayCapture>().0);
            assert_eq!(hotbar::test_occlusion(app.world()),(false,app.world().resource::<Layout>().tray_preview.as_ref().unwrap().slots.clone()),"fresh moved preview publishes its exact current rectangles");
            no_virtual_action(&app);
        }
    }
    #[test]
    fn actual_safe_publication_precedes_same_frame_editor_geometry_guard_and_raw_motion() {
        #[derive(Resource,Default)]struct Change(bool);
        fn publish(mut change:ResMut<Change>,mut safe:ResMut<SafeInsets>) {
            if std::mem::take(&mut change.0) {*safe=SafeInsets {left:16.,top:8.,right:18.,bottom:32.};}
        }
        let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(792.,363.),"classic",4);request(&mut app);
        app.init_resource::<Change>().add_systems(PreUpdate,publish.in_set(nvf_play::touch_safe_area::Publish));
        drag(&mut app,w,990,Vec2::new(400.,150.));let before=app.world().resource::<Layout>().config.tray.center;
        app.world_mut().resource_mut::<Change>().0=true;
        send(&mut app,w,990,TouchPhase::Moved,Vec2::new(100.,100.));app.update();
        let layout=app.world().resource::<Layout>();
        assert_eq!(layout.tray_safe,SafeInsets {left:16.,top:8.,right:18.,bottom:32.});
        assert!(!layout.tray_admitted && layout.tray_preview.is_none(),"same-frame inset publication retires preview before raw editor input");
        assert_eq!(layout.config.tray.center,before);assert!(mixed_hotbar_tiles(&mut app).is_empty());
        app.update();app.update();
        send(&mut app,w,990,TouchPhase::Moved,Vec2::new(200.,100.));app.update();assert_eq!(app.world().resource::<Layout>().config.tray.center,before);
        send(&mut app,w,990,TouchPhase::Ended,Vec2::new(200.,100.));app.update();
        drag(&mut app,w,991,Vec2::new(600.,170.));assert_ne!(app.world().resource::<Layout>().config.tray.center,before);
        no_virtual_action(&app);
    }
    #[test]
    fn real_size_setting_during_legacy_preview_keeps_committed_ini_and_cancelled_geometry() {
        let files=Files::new();let ini=files.0.join("settings.ini");
        let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(792.,363.),"classic",4);
        app.world_mut().resource_mut::<Layout>().path=Some(files.layout());
        assert!(!app.world().resource::<Layout>().config.tray_explicit);
        std::fs::write(&ini,"[Other]\nkeep=yes\n").unwrap();app.insert_resource(settings::SettingsFile(Some(ini.clone())));
        // Save the real registry once so an unchanged store has canonical bytes.
        let toggle=app.world().resource::<settings::Settings>().index("touch-hotbar").unwrap();
        assert_eq!(settings::flip(app.world_mut(),toggle),Some(false));assert_eq!(settings::flip(app.world_mut(),toggle),Some(true));
        let original=app.world().resource::<Layout>().config.clone();let saved=std::fs::read(&ini).unwrap();
        request(&mut app);action(&mut app,w,995,1);
        let draft=app.world().resource::<Layout>().config.tray.scale;assert!(draft>original.tray.scale);
        let index=app.world().resource::<settings::Settings>().index("touch-hotbar-size").unwrap();
        let slider=app.world().resource::<settings::Settings>().list()[index].slider.unwrap();
        assert_eq!(slider.text((slider.get)(app.world())),"100%","generic persistence must see original committed size, never toolbar draft");
        assert!(settings::set_slider(app.world_mut(),index,settings::choice_part(7,13)).is_some());
        assert_eq!(app.world().resource::<Layout>().config.tray.scale,draft,"ordinary size setter refuses to mutate the editor transaction");
        assert_eq!(std::fs::read(&ini).unwrap(),saved,"the rejected setter cannot leak draft/attempted size into legacy INI");
        assert!(!files.layout().exists(),"no JSON migration is written before explicit Save");
        action(&mut app,w,996,3);app.update();app.update();
        assert_eq!(app.world().resource::<Layout>().config,original);
        assert!(!app.world().resource::<Layout>().config.tray_explicit);
        assert_eq!(slider.text((slider.get)(app.world())),"100%");assert_eq!(std::fs::read(&ini).unwrap(),saved);
        assert!(!files.layout().exists());no_virtual_action(&app);
    }

    #[test]
    fn late_controller_preview_retirement_publishes_empty_mirrors_before_actual_core_pointer_route() {
        #[derive(Resource,Default)]struct Late(bool);
        #[derive(Debug,PartialEq)]struct Snapshot {overlay:bool,capturing:bool,rects:Vec<Rect>,key:bool,mouse:bool}
        #[derive(Resource,Default)]struct Seen(Option<Snapshot>);
        fn choose(world:&mut World) {
            if std::mem::take(&mut world.resource_mut::<Late>().0) {nvf_play::bindings::set_device(world,nvf_play::bindings::InputDevice::Controller);}
        }
        fn observe(world:&mut World) {
            let overlay=world.resource::<nvf_play::pipboy::touch_items::OverlayCapture>().0;
            let (capturing,rects)=hotbar::test_occlusion(world);
            let keys=world.resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12);
            let mouse=world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Right);
            world.resource_mut::<Seen>().0=Some(Snapshot {overlay,capturing,rects,key:keys,mouse});
        }
        let (mut app,w)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,true);
        // This helper installs the real TouchPointerPlugin/flat pointer pipeline.
        raw_key(&mut app,w,KeyCode::F12,ButtonState::Pressed);
        app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {window:w,button:MouseButton::Right,state:ButtonState::Pressed});app.update();
        request(&mut app);assert!(!hotbar::test_occlusion(app.world()).1.is_empty(),"fixture starts with actual fitted preview exclusions");
        app.init_resource::<Late>().init_resource::<Seen>()
            .add_systems(PreUpdate,choose.after(TouchInputSystems).before(nvf_play::pad::InputSwitchSystems::Switch))
            .add_systems(PreUpdate,observe.after(hotbar::PreviewPublication).before(nvf_play::pipboy::TouchPointerSet));
        app.world_mut().resource_mut::<Late>().0=true;app.update();
        assert_eq!(app.world().resource::<Seen>().0,Some(Snapshot {overlay:false,capturing:false,rects:vec![],key:true,mouse:true}),"final authority reaches the actual next core boundary and preserves hardware owners");
        assert!(app.world().resource::<Layout>().tray_preview.is_none());
        assert!(!app.world().resource::<TouchState>().ui_capture);
        assert!(!app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().pending_contact());
        assert!(mixed_hotbar_tiles(&mut app).is_empty());
    }
}
