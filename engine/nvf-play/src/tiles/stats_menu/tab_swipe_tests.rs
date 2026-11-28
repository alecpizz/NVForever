mod swipe_navigation_regressions {
    use super::*;
    use bevy::input::{InputPlugin,touch::{TouchInput,TouchPhase}};
    use crate::pipboy::{self,tab_swipe::{Kind,Key},touch_items::TouchItems};
    #[derive(Resource,Default)]struct Seen {inputs:Vec<PipBoyInput>,hints:usize}
    fn project(mut touch:ResMut<TouchItems>){touch.project(Some);}
    fn read(mut inputs:MessageReader<PipBoyInput>,mut hints:MessageReader<stats::UseHint>,mut seen:ResMut<Seen>){seen.inputs.extend(inputs.read().copied());seen.hints+=hints.read().count();}
    fn fixture()->App {
        let (mut ui,mut view,mut tiles)=(tree(),view(),StatsTiles::default());
        view.selected[2]=5;view.top[2]=3;
        let menu=run_tab(&mut ui,&snap(),&mut view,2,&mut tiles).unwrap();tiles.shown=Some(menu);
        assert_eq!((view.selected[2],view.top[2]),(5,3));
        let mut script=ScriptState::over_no_plugins();{let game=&mut script.rt.as_mut().unwrap().game;game.ui_files=Some(Box::new(MemSource::default()));game.ui_tree=ui;game.menu=Some(fnv_game::game::menus::STATS);}
        let mut pip=stats_page_up(2);pip.phase=pipboy::Phase::Up;
        let mut app=App::new();app.add_plugins((MinimalPlugins,InputPlugin))
            .init_resource::<TouchItems>().init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>().init_resource::<Seen>()
            .add_message::<PipBoyInput>().add_message::<TabHovered>().add_message::<stats::UseHint>()
            .insert_resource(script).insert_resource(view).insert_resource(tiles).insert_resource(pip)
            .insert_resource(TileMenus{stats:true,..default()});
        let camera=app.world_mut().spawn_empty().id();
        app.insert_resource(PipBoyScreen {image:Handle::default(),camera,on_model:false,pointer:None,scale:1.0})
            .add_plugins(pipboy::touch_scroll::TouchPointerPlugin)
            .add_systems(Update,(feed,pipboy::apply_input,read).chain()).add_systems(Last,project);
        app.world_mut().resource_mut::<pipboy::tab_swipe::Options>().enabled=true;
        app.update();app
    }
    fn region(app:&App,kind:Kind)->(Vec2,Option<PipBoyInput>){
        let tiles=app.world().resource::<StatsTiles>();let script=app.world().resource::<ScriptState>();let ui=&script.rt.as_ref().unwrap().game.ui_tree;
        let r=touch_navigation(tiles,ui).into_iter().find(|r|r.kind==kind && (kind==Kind::Sections || r.rect.center().x>=120.0)).expect("actual measured STATS title/tab text");
        let Key::Native(tile)=r.key else {panic!("native region identity")};
        let direct=match stats_tile(ui,&tiles.written.paths.unwrap(),&tiles.written.rows,tile){Some(StatsTile::Tab(i))=>Some(PipBoyInput::Tab(i)),_=>None};
        (r.rect.center(),direct)
    }
    fn event(app:&mut App,phase:TouchPhase,p:Vec2,ms:u64){assert!(app.world_mut().resource_mut::<TouchItems>().event(&TouchInput{window:Entity::PLACEHOLDER,id:410,phase,position:p,force:None},ms));}
    fn flush(app:&mut App){for _ in 0..6 {app.update();}}
    fn swipe(app:&mut App,p:Vec2,delta:Vec2){event(app,TouchPhase::Started,p,0);app.update();app.update();event(app,TouchPhase::Moved,p+delta,30);event(app,TouchPhase::Ended,p+delta,80);flush(app);}
    #[test]
    fn native_stats_title_and_tab_swipes_preserve_selection_and_ordinary_tap_route(){
        for kind in [Kind::Sections,Kind::Subtabs] {for mode in 0..4 {
            let mut app=fixture();let (point,direct)=region(&app,kind);
            let selected=app.world().resource::<StatsView>().selected;let top=app.world().resource::<StatsView>().top;
            event(&mut app,TouchPhase::Started,point,0);app.update();app.update();
            assert_eq!((app.world().resource::<PipBoy>().page,app.world().resource::<PipBoy>().tab()),(Page::Stats,2),"press is provisional");
            match mode {
                0=>{event(&mut app,TouchPhase::Moved,point+Vec2::X*60.0,30);event(&mut app,TouchPhase::Ended,point+Vec2::X*60.0,80);},
                1=>event(&mut app,TouchPhase::Ended,point,80),
                2=>{event(&mut app,TouchPhase::Moved,point+Vec2::Y*60.0,30);event(&mut app,TouchPhase::Ended,point,80);},
                _=>{event(&mut app,TouchPhase::Moved,point+Vec2::X*60.0,30);event(&mut app,TouchPhase::Canceled,point+Vec2::X*60.0,80);},
            }
            flush(&mut app);
            let expected=if mode==0 {Some(if kind==Kind::Sections {PipBoyInput::Page(Page::Data)}else{PipBoyInput::StepTab(-1)})}else if mode==1 {direct}else{None};
            assert_eq!(app.world().resource::<Seen>().inputs,expected.into_iter().collect::<Vec<_>>());
            assert_eq!(app.world().resource::<Seen>().hints,0);
            assert_eq!((app.world().resource::<StatsView>().selected,app.world().resource::<StatsView>().top),(selected,top));
        }}
    }
    #[test]
    fn native_stats_swipe_away_and_return_repaints_original_selection_and_scroll(){
        let mut app=fixture();let point=region(&app,Kind::Subtabs).0;swipe(&mut app,point,Vec2::X*60.0);
        assert_eq!(app.world().resource::<PipBoy>().tab(),1);
        let mut tiles=app.world_mut().remove_resource::<StatsTiles>().unwrap();let mut view=app.world_mut().remove_resource::<StatsView>().unwrap();
        {let mut script=app.world_mut().resource_mut::<ScriptState>();let menu=run_tab(&mut script.rt.as_mut().unwrap().game.ui_tree,&snap(),&mut view,1,&mut tiles).unwrap();tiles.shown=Some(menu);}
        app.insert_resource(tiles).insert_resource(view);
        let point=region(&app,Kind::Subtabs).0;swipe(&mut app,point,-Vec2::X*60.0);
        assert_eq!(app.world().resource::<PipBoy>().tab(),2);
        let mut tiles=app.world_mut().remove_resource::<StatsTiles>().unwrap();let mut view=app.world_mut().remove_resource::<StatsView>().unwrap();
        {let mut script=app.world_mut().resource_mut::<ScriptState>();let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;let menu=run_tab(ui,&snap(),&mut view,2,&mut tiles).unwrap();tiles.shown=Some(menu);
            let p=paths(ui,2,None).unwrap();assert_eq!(list::value_shown(ui,p.list()),Some(3.0));}
        assert_eq!((view.selected[2],view.top[2]),(5,3));assert_eq!(app.world().resource::<Seen>().inputs,vec![PipBoyInput::StepTab(-1),PipBoyInput::StepTab(1)]);
        assert_eq!(app.world().resource::<Seen>().hints,0);
    }
}
