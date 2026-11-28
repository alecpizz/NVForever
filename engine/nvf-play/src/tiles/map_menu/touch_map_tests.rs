// Actual native DATA prepare/settle/write/refit, fed by the shared raw map bridge.
mod map_touch_consumer_routes {
    use super::*;
    use crate::pipboy::data::touch_map::tests as mobile;
    use bevy::input::touch::TouchPhase;
    fn fixture(tab:usize)->(App,Entity) {
        let (mut app,window)=mobile::scene(tab,1.0,1.0);
        let bodies={let w=app.world_mut();let mut q=w.query_filtered::<Entity,With<crate::pipboy::PageBody>>();q.iter(w).collect::<Vec<_>>()};
        for entity in bodies {app.world_mut().despawn(entity);}
        let mut script=ScriptState::over_no_plugins();{
            let game=&mut script.rt.as_mut().unwrap().game;
            game.ui_files=Some(Box::new(MemSource::default()));game.ui_tree=tree_changed(|x|with_local(with_world(x)));
            game.menu=Some(fnv_game::game::menus::MAP);
        }
        app.insert_resource(script).insert_resource(TileMenus{map:true,..default()})
            .init_resource::<MapTiles>().init_resource::<crate::tiles::draw::TileFonts>()
            .init_resource::<crate::pause::PlayClock>()
            .init_resource::<crate::tiles::menus::TileMenuBlur>()
            .add_systems(Update,data::prepare.after(crate::pipboy::flat_pointer).before(data::draw))
            .add_systems(Update,settle.after(data::prepare).before(data::draw))
            // Only the real native painter publishes MapTiles::shown after accepting ready.
            .add_systems(Update,crate::tiles::draw::draw.after(settle).after(data::draw));
        mobile::settle(&mut app);
        {
            let tiles=app.world().resource::<MapTiles>();
            assert!(tiles.ready.is_some(),"native map settle rejected tab {tab}: {}",tiles.written.reason);
        }
        assert!(app.world().resource::<MapTiles>().shown.is_some(),"the real native page must be accepted, not a fallback facade");
        let measured=app.world().resource::<MapTiles>().written.counted.map.as_ref().unwrap().0;
        assert_eq!(mobile::shown(&app).window,measured);assert_ne!(measured,mobile::handbuilt_window());
        (app,window)
    }
    #[test]
    fn actual_native_local_and_world_maps_pan_pinch_and_publish_the_same_clipped_model_without_actions() {
        for tab in [data::LOCAL,data::WORLD] {
            let (mut app,window)=fixture(tab);let before=mobile::shown(&app);
            let point=mobile::raw_point(&mut app,before.window.center());
            mobile::step_edge(&mut app,window,301,TouchPhase::Started,point);
            mobile::step_edge(&mut app,window,301,TouchPhase::Moved,point+Vec2::new(36.,18.));
            mobile::step_edge(&mut app,window,301,TouchPhase::Ended,point+Vec2::new(36.,18.));
            let after=mobile::shown(&app);assert_ne!(before,after);
            let world=app.world().resource::<MapTiles>().written.shown.as_ref().unwrap().world.as_ref().unwrap();
            assert_eq!(world.view,after,"native painter and input share the actual refitted viewport");
            let raw_centre=mobile::raw_point(&mut app,after.window.center());
            mobile::edge(&mut app,window,302,TouchPhase::Started,raw_centre-Vec2::X*40.);
            mobile::edge(&mut app,window,303,TouchPhase::Started,raw_centre+Vec2::X*40.);app.update();
            mobile::step_edge(&mut app,window,302,TouchPhase::Moved,raw_centre-Vec2::X*80.);
            mobile::step_edge(&mut app,window,303,TouchPhase::Moved,raw_centre+Vec2::X*80.);
            let view=mobile::shown(&app);
            assert!((mobile::magnification(&app,tab)-2.0).abs()<1e-4);
            let tiles=app.world().resource::<MapTiles>();let native=tiles.written.shown.as_ref().unwrap().world.as_ref().unwrap();
            assert_eq!(native.view,view);assert!((native.mag-2.0).abs()<1e-4);
            let script=app.world().resource::<ScriptState>();let ui=&script.rt.as_ref().unwrap().game.ui_tree;
            let paths=if tab==data::LOCAL {&LOCAL_PATHS}else{&WORLD_PATHS};
            assert!((at(ui,paths.picture,"_Magnification")-2.0).abs()<1e-4,"the actual native tree received the new zoom");
            mobile::edge(&mut app,window,302,TouchPhase::Ended,raw_centre-Vec2::X*80.);
            mobile::edge(&mut app,window,303,TouchPhase::Ended,raw_centre+Vec2::X*80.);app.update();mobile::settle(&mut app);
            mobile::quiet(&app);assert!(app.world().resource::<MapTiles>().shown.is_some());
        }
    }
}
