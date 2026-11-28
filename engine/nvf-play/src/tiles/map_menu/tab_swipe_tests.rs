mod swipe_navigation_regressions {
    use super::*;
    use bevy::input::{InputPlugin,touch::{TouchInput,TouchPhase}};
    use crate::pipboy::{self,tab_swipe::{Kind,Key},touch_items::TouchItems};
    #[derive(Resource,Default)]struct Seen {inputs:Vec<PipBoyInput>,acts:Vec<Act>}
    fn project(mut touch:ResMut<TouchItems>){touch.project(Some);}
    fn read(mut inputs:MessageReader<PipBoyInput>,mut data:ResMut<DataState>,mut seen:ResMut<Seen>){seen.inputs.extend(inputs.read().copied());seen.acts.extend(data.take_touch_test_actions());}
    fn fixture()->App {
        let (mut ui,mut tiles)=(tree(),MapTiles::default());
        let menu=run(&mut ui,Some(shown(12,5,0)),&mut tiles).unwrap();tiles.shown=Some(menu);
        tiles.written.views[ListKind::Quests.index()].scroll_to(3.0);run(&mut ui,None,&mut tiles).unwrap();
        assert_eq!(tiles.written.views[ListKind::Quests.index()].value,3.0);
        let mut script=ScriptState::over_no_plugins();{let game=&mut script.rt.as_mut().unwrap().game;game.ui_files=Some(Box::new(MemSource::default()));game.ui_tree=ui;game.menu=Some(fnv_game::game::menus::MAP);}
        let mut pip=PipBoy::default();pip.raise();pip.page=Page::Data;pip.phase=pipboy::Phase::Up;
        let mut app=App::new();app.add_plugins((MinimalPlugins,InputPlugin))
            .init_resource::<TouchItems>().init_resource::<DataState>().init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>().init_resource::<Seen>()
            .add_message::<PipBoyInput>().add_message::<TabHovered>()
            .insert_resource(script).insert_resource(tiles).insert_resource(pip).insert_resource(TileMenus{map:true,..default()});
        app.world_mut().resource_mut::<PipBoy>().tabs[Page::Data.index()]=data::QUESTS;
        let camera=app.world_mut().spawn_empty().id();
        app.insert_resource(PipBoyScreen {image:Handle::default(),camera,on_model:false,pointer:None,scale:1.0})
            .add_plugins(pipboy::touch_scroll::TouchPointerPlugin)
            .add_systems(Update,(feed,pipboy::apply_input,read).chain()).add_systems(Last,project);
        app.world_mut().resource_mut::<pipboy::tab_swipe::Options>().enabled=true;
        app.update();app
    }
    fn region(app:&App,kind:Kind)->(Vec2,Option<PipBoyInput>){
        let tiles=app.world().resource::<MapTiles>();let script=app.world().resource::<ScriptState>();let ui=&script.rt.as_ref().unwrap().game.ui_tree;
        let r=touch_navigation(tiles,ui).into_iter().find(|r|r.kind==kind && (kind==Kind::Sections || r.rect.center().x>=120.0)).expect("actual measured DATA title/tab text");
        let Key::Native(tile)=r.key else {panic!("native region identity")};
        let direct=match data_tile(ui,&tiles.written,tile){Some(DataTile::Tab(i))=>Some(PipBoyInput::Tab(i)),_=>None};
        (r.rect.center(),direct)
    }
    fn event(app:&mut App,phase:TouchPhase,p:Vec2,ms:u64){assert!(app.world_mut().resource_mut::<TouchItems>().event(&TouchInput{window:Entity::PLACEHOLDER,id:420,phase,position:p,force:None},ms));}
    fn flush(app:&mut App){for _ in 0..6 {app.update();}}
    fn swipe(app:&mut App,p:Vec2,delta:Vec2){event(app,TouchPhase::Started,p,0);app.update();app.update();event(app,TouchPhase::Moved,p+delta,30);event(app,TouchPhase::Ended,p+delta,80);flush(app);}
    #[test]
    fn native_data_title_and_tab_swipes_never_activate_rows_buttons_or_markers(){
        for kind in [Kind::Sections,Kind::Subtabs] {for mode in 0..4 {
            let mut app=fixture();let (point,direct)=region(&app,kind);
            event(&mut app,TouchPhase::Started,point,0);app.update();app.update();
            assert_eq!((app.world().resource::<PipBoy>().page,app.world().resource::<PipBoy>().tab()),(Page::Data,data::QUESTS));
            let delta=if kind==Kind::Sections {Vec2::X*60.0}else{-Vec2::X*60.0};
            match mode {
                0=>{event(&mut app,TouchPhase::Moved,point+delta,30);event(&mut app,TouchPhase::Ended,point+delta,80);},
                1=>event(&mut app,TouchPhase::Ended,point,80),
                2=>{event(&mut app,TouchPhase::Moved,point+Vec2::Y*60.0,30);event(&mut app,TouchPhase::Ended,point,80);},
                _=>{event(&mut app,TouchPhase::Moved,point+delta,30);event(&mut app,TouchPhase::Canceled,point+delta,80);},
            }
            flush(&mut app);
            let expected=if mode==0 {Some(if kind==Kind::Sections {PipBoyInput::Page(Page::Items)}else{PipBoyInput::StepTab(1)})}else if mode==1 {direct}else{None};
            assert_eq!(app.world().resource::<Seen>().inputs,expected.into_iter().collect::<Vec<_>>());
            assert!(app.world().resource::<Seen>().acts.is_empty(),"navigation cannot invoke native DATA action");
            assert_eq!(app.world().resource::<MapTiles>().written.views[ListKind::Quests.index()].value,3.0);
        }}
    }
    #[test]
    fn native_data_swipe_return_keeps_each_real_lists_scroll_and_painted_selection(){
        let mut app=fixture();let point=region(&app,Kind::Subtabs).0;swipe(&mut app,point,-Vec2::X*60.0);
        assert_eq!(app.world().resource::<PipBoy>().tab(),data::NOTES);
        let mut tiles=app.world_mut().remove_resource::<MapTiles>().unwrap();
        {let mut script=app.world_mut().resource_mut::<ScriptState>();let menu=run(&mut script.rt.as_mut().unwrap().game.ui_tree,Some(misc(ListKind::Notes,12,4,Pane::None)),&mut tiles).unwrap();tiles.shown=Some(menu);}
        tiles.written.views[ListKind::Notes.index()].scroll_to(2.0);
        {let mut script=app.world_mut().resource_mut::<ScriptState>();run(&mut script.rt.as_mut().unwrap().game.ui_tree,None,&mut tiles).unwrap();}
        app.insert_resource(tiles);
        let point=region(&app,Kind::Subtabs).0;swipe(&mut app,point,Vec2::X*60.0);
        assert_eq!(app.world().resource::<PipBoy>().tab(),data::QUESTS);
        let mut tiles=app.world_mut().remove_resource::<MapTiles>().unwrap();
        {let mut script=app.world_mut().resource_mut::<ScriptState>();let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;let menu=run(ui,Some(shown(12,5,0)),&mut tiles).unwrap();tiles.shown=Some(menu);
            assert_eq!(ui.find_tile(QUESTS_LIST).and_then(|l|list::value_shown(ui,l)),Some(3.0));}
        assert_eq!(tiles.written.shown.as_ref().unwrap().selected,Some(5));
        assert_eq!(tiles.written.views[ListKind::Quests.index()].value,3.0);assert_eq!(tiles.written.views[ListKind::Notes.index()].value,2.0);
        assert_eq!(app.world().resource::<Seen>().inputs,vec![PipBoyInput::StepTab(1),PipBoyInput::StepTab(-1)]);
        assert!(app.world().resource::<Seen>().acts.is_empty());
    }
}
