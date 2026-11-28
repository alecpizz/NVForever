mod manual_scroll_roundtrip {
    use super::*;
    use crate::menu::{model::{Category,Command,MenuModel,RowKey,Side},ActiveMenu,MenuInput};
    use crate::pipboy::scrollbar::{ItemsScroll,ListScrollRequest,ScrollBy,ScrollbarPlugin};
    const PLAYER:usize=Side::Player as usize;
    #[derive(Resource,Default)]
    struct PendingManual {requested:bool,observed:Option<(usize,usize)>}
    fn shared_scroll_before_navigation(world:&mut World) {
        if !std::mem::take(&mut world.resource_mut::<PendingManual>().requested) {return}
        let request=ListScrollRequest::new(world.resource::<PipBoy>(),world.resource::<pipboy::PipBoyScreen>(),ScrollBy::Lines(3)).unwrap();
        assert!(request.apply(world),"actual shared scrollbar authority must admit this hand-built contact");
        let view=world.resource::<ItemsScroll>().position().unwrap();
        let fallback=world.resource::<ActiveMenu>().0.as_ref().unwrap().scroll[PLAYER];
        world.resource_mut::<PendingManual>().observed=Some((view,fallback));
    }
    fn apply_menu(mut messages:MessageReader<MenuInput>,mut active:ResMut<ActiveMenu>,mut sim:ResMut<crate::sim::SimState>) {
        for message in messages.read() {if let Some(model)=active.0.as_mut() {model.apply(&mut sim.world,message.0);}}
    }
    fn model(app:&App)->&MenuModel {app.world().resource::<ActiveMenu>().0.as_ref().unwrap()}
    fn fixture()->(App,Entity) {
        let (mut app,window)=scene(1.0);
        let mut sim=crate::sim::test_world::sim((0..24).map(|i| {
            let mut base=crate::sim::test_world::item_base(0xF500+i);base.name=Some(format!("Authored aid {i:02}"));base
        }).collect(),vec![]);
        for i in 0..24 {sim.world.add_item(fnv_sim::FormId::PLAYER_REF,fnv_sim::FormId(0xF500+i),3).unwrap();}
        let mut menu=MenuModel::open_inventory(&mut sim.world).unwrap();menu.apply(&mut sim.world,Command::SetCategory(Side::Player,Category::Aid));
        app.insert_resource(sim).insert_resource(ActiveMenu(Some(menu)))
            .init_resource::<pipboy::repair::PipRepair>().init_resource::<pipboy::item_mod::PipItemMod>()
            .init_resource::<pipboy::KeysTaken>().add_systems(First,pipboy::clear_keys_taken)
            .init_resource::<pipboy::hotkey_panel::HotkeyHold>().init_resource::<crate::hotkeys::Hotkeys>()
            .init_resource::<ItemsScroll>().init_resource::<PendingManual>().add_message::<MenuInput>()
            .add_plugins(ScrollbarPlugin)
            .add_systems(Update,shared_scroll_before_navigation.before(Systems::Dispatch))
            .add_systems(Update,(pipboy::items::keys,apply_menu,pipboy::items::draw).chain().after(pipboy::frame::draw).after(Systems::Restore));
        flush(&mut app);
        assert_eq!(model(&app).lists[PLAYER].rows.len(),24);
        assert!(app.world().resource::<ItemsScroll>().matches(model(&app)),"production painter must establish the scroll context");
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(0));
        let world=app.world_mut();let mut bodies=world.query::<(&pipboy::PageBody,&Children)>();
        assert!(bodies.iter(world).any(|(body,children)|body.page==Page::Items && !children.is_empty()),"actual ITEMS painter must produce its row/hit/scrollbar children");
        (app,window)
    }
    #[test]
    fn actual_handbuilt_scroll_then_same_frame_tab_swipe_restores_painted_position_and_row_identity() {
        let (mut app,window)=fixture();
        let selected=RowKey::at(&model(&app).lists[PLAYER].rows,model(&app).selected[PLAYER]).unwrap();
        let p=point(&mut app,Kind::Subtabs);
        event(&mut app,window,TouchPhase::Started,p,0);app.update();app.update();
        event(&mut app,window,TouchPhase::Moved,p-Vec2::X*60.0,30);app.update();app.update();
        event(&mut app,window,TouchPhase::Ended,p-Vec2::X*60.0,80);app.update();
        // Last projected Ended; next PreUpdate creates the real nav request. A
        // shared scrollbar command may update the independent view before paint.
        app.world_mut().resource_mut::<PendingManual>().requested=true;app.update();flush(&mut app);
        assert_eq!(app.world().resource::<PendingManual>().observed,Some((3,0)),"fixture must expose actual view ahead of model fallback, not merely mutate the model");
        assert_eq!(app.world().resource::<PipBoy>().tab(),3);
        assert_eq!(model(&app).category[PLAYER],Category::Misc);
        assert!(app.world().resource::<ItemsScroll>().matches(model(&app)));
        let p=point(&mut app,Kind::Subtabs);swipe(&mut app,window,p,Vec2::X*60.0);
        assert_eq!(app.world().resource::<PipBoy>().tab(),2);assert_eq!(model(&app).category[PLAYER],Category::Aid);
        assert_eq!(RowKey::at(&model(&app).lists[PLAYER].rows,model(&app).selected[PLAYER]),Some(selected),"navigation restores the original gameplay row independently of view movement");
        assert_eq!(app.world().resource::<ItemsScroll>().position(),Some(3),"actual ITEMS painter must restore the manually scrolled first row");
        assert!(app.world().resource::<ItemsScroll>().matches(model(&app)));
        assert_eq!(model(&app).scroll[PLAYER],3,"the next real paint follows the restored view rather than its stale fallback");
        assert_eq!(app.world().resource::<Seen>().0.len(),2,"exactly one native tab action per deliberate swipe");
        assert!(app.world().resource::<crate::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
        for i in 0..24 {assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF,fnv_sim::FormId(0xF500+i)).unwrap(),3);}
    }
}
