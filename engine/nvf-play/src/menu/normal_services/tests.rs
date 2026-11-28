//! Raw finger -> existing native/HB geometry -> original services consumers.
//! Authored CPU controls; source-specific results do not establish renderer/device acceptance.
use super::*;
use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},camera::{ComputedCameraValues,RenderTargetInfo,
    RenderTarget,visibility::VisibilityPlugin},input::{InputPlugin,InputSystems,touch::Touches},
    ui::{ComputedUiTargetCamera,ComputedUiRenderTargetInfo,ui_surface::UiSurface,update::propagate_ui_target_cameras},
    window::{PrimaryWindow,WindowRef,CursorMoved}};
use crate::{script::ScriptState,sim::{SimState,test_world},menu::{ActiveMenu,model::Command}};
use fnv_sim::{FormId,forms::{BaseData,ItemKind},inventory::Extra};
use bevy::ecs::system::RunSystemOnce;
use bevy::state::app::StatesPlugin;
const PLAYER:FormId=FormId::PLAYER_REF;
const VENDOR:FormId=FormId(0xFC101);
const ARMOR:FormId=FormId(0xFC110);
const INPUT:FormId=FormId(0xFC120);
const OUTPUT:FormId=FormId(0xFC121);
const CATEGORY:FormId=FormId(0xFC130);
const CAPS:FormId=fnv_game::repair::CAPS;

#[derive(Resource)]
struct Owner {frame:Frame,kind:Kind}
fn raw_owner(owner:Res<Owner>,touches:Res<Touches>,mut events:MessageReader<TouchInput>,
    preferences:crate::maintenance_touch::Requests,mut bridge:Bridge,
    mut capture:ResMut<crate::ui_pointer::TouchUiCapture>) {
    capture.begin_frame();
    let neutral=touches.iter().next().is_none() && !touches.any_just_released() && !touches.any_just_canceled();
    bridge.prepare(Some(owner.kind),Frame {neutral,admitted:preferences.enabled(),..owner.frame});
    for e in events.read(){bridge.event(e);}
    for t in touches.iter().chain(touches.iter_just_pressed()).chain(touches.iter_just_released())
        .chain(touches.iter_just_canceled()) {
        if bridge.captures_touch(owner.frame.window,t.id()){capture.block(t.id());}
    }
    capture.suppress_pointer(bridge.visible());
}
fn authored_world()->SimState {
    let mut bases=Vec::new();
    for i in 0..12 {
        let mut base=test_world::item_base(ARMOR.0+i);
        base.name=Some(format!("Repairable armor {i:02}"));
        if let BaseData::Item(item)=&mut base.data {item.kind=ItemKind::Armor;
            item.health=Some(100);item.value=Some(100);item.armor_dt=Some(5.0);}
        bases.push(base);
    }
    for id in [CAPS,INPUT,OUTPUT] {let mut b=test_world::item_base(id.0);
        b.name=Some(format!("Authored material {}",id.0));bases.push(b);}
    let mut vendor=test_world::item_base(VENDOR.0-1);vendor.sig = *b"NPC_";
    vendor.data=BaseData::Actor(fnv_sim::forms::ActorBase {entries:vec![],attributes:None});bases.push(vendor);
    let mut placed=test_world::placed(VENDOR.0,VENDOR.0-1);placed.sig = *b"ACHR";
    let mut sim=test_world::sim(bases,vec![placed]);
    for i in 0..12 {let id=FormId(ARMOR.0+i);sim.world.add_item(PLAYER,id,1).unwrap();
        assert!(sim.world.inventory_mut(PLAYER).unwrap().set_condition_one(id,Extra::default(),0.2));}
    sim.world.equip_at(PLAYER,ARMOR,fnv_sim::inventory::Pick::row(Some(0.2),0)).unwrap();
    sim.world.add_item(PLAYER,CAPS,10_000).unwrap();sim.world.add_item(PLAYER,INPUT,12).unwrap();
    sim
}
fn authored_script(sim:&mut SimState,kind:Kind)->ScriptState {
    let schemas=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let functions=schemas.join("script/functions.json");
    let json=serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
    let mut data=fnv_game::data::GameData::build(&fnv_esm::LoadOrder::new(&[]).unwrap(),
        &fnv_esm::Schema::load_dir(schemas.join("fnv")).unwrap(),
        fnv_script::FunctionTable::load(&functions).unwrap(),&json,sim.refs);
    data.recipe_categories.insert(CATEGORY,fnv_game::crafting::RecipeCategoryDef {name:"Authored workbench".into(),subcategory:false});
    for i in 0..12 {let form=FormId(0xFC200+i);data.recipes.insert(form,fnv_game::crafting::RecipeDef {
        form,name:format!("Workbench recipe {i:02}"),category:Some(CATEGORY),
        ingredients:vec![(INPUT,2)],outputs:vec![(OUTPUT,1)],..default()});}
    let mut script=ScriptState::over_no_plugins();script.rt.as_mut().unwrap().game.data=Box::leak(Box::new(data));
    let ctx=fnv_game::functions::Ctx {unit:"ordinary-services-fixture".into(),opcode:0,on:Some(VENDOR),
        self_ref:None,action_ref:None,quest:None,inst:None,in_condition:false};
    crate::script::with_world(script.rt.as_mut().unwrap(),sim,|rt| {
        fnv_game::functions::set_actor_value_current(&mut rt.game,VENDOR,fnv_game::actor_values::REPAIR,80.0).unwrap();
        match kind {
            Kind::Vendor=>{fnv_game::functions::misc::show_repair_menu(&mut rt.game,&ctx,&fnv_game::functions::Args(&[])).unwrap();}
            Kind::Workbench=>{let args=[fnv_script::vm::ArgValue::Value(fnv_script::vm::Value::Ref(CATEGORY.0))];
                fnv_game::functions::misc::show_recipe_menu(&mut rt.game,&ctx,&fnv_game::functions::Args(&args)).unwrap();}
        }
    });
    script
}
fn unsettled_scene(kind:Kind,native:bool)->App {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(||unsettled_scene_inner(kind,native))) {
        Ok(app)=>app,
        Err(payload)=>{
            let text=payload.downcast_ref::<&str>().copied()
                .or_else(||payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-string panic payload>");
            eprintln!("normal services setup panic: kind={kind:?} native={native} payload={text}");
            std::panic::resume_unwind(payload);
        }
    }
}
fn unsettled_scene_inner(kind:Kind,native:bool)->App {
    let mut sim=authored_world();let mut script=authored_script(&mut sim,kind);
    if native {
        let ui=match kind {
            Kind::Vendor=>crate::tiles::repair_services_menu::tests::fixture(|s|s),
            Kind::Workbench=>crate::tiles::recipe_menu::tests::fixture(|s|s
                .replace("<rect name=\"RM_ItemsRect\"><x>30</x>","<rect name=\"RM_ItemsRect\"><x>30</x><y>120</y>")
                .replace("<rect name=\"RM_ContainerRect\"><x>400</x>","<rect name=\"RM_ContainerRect\"><x>400</x><y>120</y>")
                .replace("<rect name=\"RM_ButtonRect\">","<rect name=\"RM_ButtonRect\"><y>350</y>")
                .replace("<image name=\"RM_ButtonX\">","<image name=\"RM_ButtonX\"><x>30</x><width>100</width><height>35</height><target>1</target>")
                .replace("<image name=\"RM_ButtonB\">","<image name=\"RM_ButtonB\"><x>400</x><width>100</width><height>35</height><target>1</target>")),
        };
        let game=&mut script.rt.as_mut().unwrap().game;game.ui_tree=ui;
        game.ui_files=Some(Box::new(fnv_ui::MemSource::default()));
    }
    let mut app=App::new();
    app.add_plugins((MinimalPlugins,StatesPlugin,InputPlugin,bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin,bevy::text::TextPlugin))
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_plugins((VisibilityPlugin,HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),crate::ui_pointer::UiPointerPlugin))
        .init_resource::<UiSurface>().init_resource::<UiStack>().init_resource::<crate::ui_pointer::TouchUiCapture>()
        .insert_state(crate::PlayState::Playing).insert_resource(sim).insert_resource(script)
        .insert_resource(ActiveMenu(None)).insert_resource(crate::settings::SettingsFile(None))
        .init_resource::<UiScale>().init_resource::<crate::touch_safe_area::SafeInsets>()
        .init_resource::<crate::bindings::KeyboardCapture>().init_resource::<crate::free_cam::FreeCam>()
        .init_resource::<crate::pause::Paused>().init_resource::<crate::pad::PlatformInputGate>()
        .init_resource::<crate::bindings::Bindings>().init_resource::<crate::menu::PromptKeys>()
        .init_resource::<crate::menu::FreshMenu>().init_resource::<crate::menu::WheelHold>()
        .init_resource::<crate::pause::PlayClock>().init_resource::<crate::pad::PadGlyphs>()
        .init_resource::<Assets<Image>>().init_resource::<crate::tiles::draw::TileFonts>()
        .init_resource::<crate::tiles::menus::TileMenuBlur>()
        .init_resource::<crate::tiles::input::MessageClick>().init_resource::<crate::tiles::input::WaitSliderCheck>()
        .init_resource::<crate::tiles::container_menu::ContainerTiles>().init_resource::<crate::tiles::container_menu::TileWheelTitle>()
        .init_resource::<crate::tiles::recipe_menu::RecipeTiles>().init_resource::<crate::tiles::repair_services_menu::RepairServicesTiles>()
        .insert_resource(crate::tiles::vanilla::TileMenus {recipe:native,repair_services:native,..default()})
        .add_message::<crate::menu::MenuInput>().add_message::<crate::wait::WaitInput>()
        .add_message::<crate::tutorial_menu::TutorialInput>().add_message::<CursorMoved>()
        .add_message::<crate::ui_sounds::UiSound>();
    install(&mut app);
    app.add_plugins((crate::repair::RepairPlugin,crate::crafting::CraftingPlugin,crate::maintenance_touch::MaintenanceTouchPlugin));
    crate::menu_readability::register(&mut app,crate::pause::GROUP_MENUS);
    let setting=app.world().resource::<crate::settings::Settings>().index(crate::maintenance_touch::SETTING_KEY).unwrap();
    if !app.world().resource::<crate::maintenance_touch::State>().enabled() {
        assert_eq!(crate::settings::flip(app.world_mut(),setting),Some(true));
    }
    let window=app.world_mut().spawn((Window {resolution:(1280,720).into(),focused:true,..default()},PrimaryWindow)).id();
    let camera=app.world_mut().spawn((Camera2d,bevy::ui::IsDefaultUiCamera,Camera {
        computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {physical_size:UVec2::new(1280,720),scale_factor:1.0}),..default()},..default()
    },RenderTarget::Window(WindowRef::Entity(window)))).id();
    app.insert_resource(Owner {kind,frame:Frame {window,camera,size:Vec2::new(1280.0,720.0),safe:default(),
        density:1.0,ui_scale:1.0,admitted:true,fresh:false,neutral:true}});
    app.add_systems(PreUpdate,(raw_owner.after(InputSystems).before(bevy::ui::UiSystems::Focus),
        bevy::ui::ui_focus_system.in_set(bevy::ui::UiSystems::Focus)));
    app.add_systems(Update,crate::tiles::input::feed.before(crate::PlaySet::Ui));
    if native {
        crate::tiles::recipe_menu::install_normal_fixture_writer(&mut app);
        crate::tiles::repair_services_menu::install_normal_fixture_writer(&mut app);
        app.add_systems(Update,crate::tiles::draw::draw.after(crate::PlaySet::Ui));
    }
    app.add_systems(PostUpdate,(ApplyDeferred,propagate_ui_target_cameras,
        bevy::ui::sync_font_size_to_em_size,bevy::ui::widget::measure_text_system,bevy::ui::ui_layout_system,
        bevy::ui::update::update_clipping_system,ApplyDeferred,
        crate::menu::barter_touch::tests::normal_ui_stack).chain()
        .after(bevy::text::load_font_assets_into_font_collection)
        .after(bevy::text::detect_text_needs_rerender));
    app.configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default()
        .after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
    app.configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default()
        .after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
    app
}
fn scene(kind:Kind,native:bool)->App {
    let mut app=unsettled_scene(kind,native);
    for boot_frame in 1..=7 {
        if let Err(payload)=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||app.update())) {
            let text=payload.downcast_ref::<&str>().copied()
                .or_else(||payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-string panic payload>");
            eprintln!("normal services boot App.update panic: kind={kind:?} native={native} boot_frame={boot_frame} payload={text}");
            std::panic::resume_unwind(payload);
        }
    }
    assert_eq!(*app.world().resource::<State<crate::PlayState>>().get(),crate::PlayState::Menu);
    assert!(!app.world().resource::<crate::maintenance_touch::State>().visible(),"ordinary service route never prepares replacement sheet");
    assert!(app.world().resource::<NormalServices>().visible());
    let g=guard(&app);assert_eq!(g.kind(),kind);
    let native_ready=match kind {Kind::Vendor=>app.world().resource::<crate::tiles::repair_services_menu::RepairServicesTiles>().menu.is_some(),
        Kind::Workbench=>app.world().resource::<crate::tiles::recipe_menu::RecipeTiles>().menu.is_some()};
    assert_eq!(native_ready,native,"actual writer selected authored native versus ordinary handbuilt presentation");
    app
}
fn guard(app:&App)->Guard {match app.world().resource::<Owner>().kind {
    Kind::Vendor=>app.world().resource::<crate::repair::RepairServices>().normal_guard().unwrap(),
    Kind::Workbench=>app.world().resource::<crate::crafting::RecipeMenu>().normal_guard().unwrap(),
}}
fn count(app:&App,who:FormId,item:FormId)->i32 {
    app.world().resource::<SimState>().world.peek(who).and_then(|s|s.inventory.as_ref()).map_or(0,|i|i.count(item))
}
fn edge(app:&mut App,id:u64,phase:TouchPhase,at:Vec2) {
    let window=app.world().resource::<Owner>().frame.window;
    app.world_mut().write_message(TouchInput {window,id,phase,position:at,force:None});
}
fn find_hit_point(app:&mut App,wanted:impl Fn(Action)->bool)->Option<Vec2> {
    let f=app.world().resource::<Owner>().frame;let g=guard(app);
    let native_points={
        let ui=&app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
        let name=match g.kind(){Kind::Vendor=>"RepairServicesMenu",Kind::Workbench=>"RecipeMenu"};
        ui.menu(name).map_or_else(Vec::new,|menu|fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure)
            .into_iter().filter_map(|c| {let r=c.region();(r.width>0.0 && r.height>0.0).then(||
                crate::interface::to_menu_frame(Vec2::new((r.left+r.width/2.0) as f32,(r.top+r.height/2.0) as f32)
                    *(f.size.y/960.0),f.size,Vec2::splat(0.5))) }).collect())
    };
    for p in native_points {
            let found=app.world_mut().run_system_once({let g=g.clone();move |geometry:Geometry,script:Res<ScriptState>| {
                geometry.hit(Some(&script.rt.as_ref().unwrap().game),f,&g,p).map(|h|h.action)
            }}).unwrap();
            if found.is_some_and(&wanted){return Some(p)}
    }
    let mut q=app.world_mut().query::<(&ComputedNode,&UiGlobalTransform)>();
    let points:Vec<_>=q.iter(app.world()).filter_map(|(n,t)|(n.size().min_element()>0.0).then_some(t.translation/f.density)).collect();
    for p in points {
        let found=app.world_mut().run_system_once({let g=g.clone();move |geometry:Geometry,script:Res<ScriptState>| {
            geometry.hit(Some(&script.rt.as_ref().unwrap().game),f,&g,p).map(|h|h.action)
        }}).unwrap();
        if found.is_some_and(&wanted){return Some(p)}
    }
    None
}
#[track_caller]
fn hit_point(app:&mut App,wanted:impl Fn(Action)->bool)->Vec2 {
    if let Some(point)=find_hit_point(app,wanted){return point}
    let f=app.world().resource::<Owner>().frame;let g=guard(app);
    let native=match g.kind() {Kind::Vendor=>app.world().resource::<crate::tiles::repair_services_menu::RepairServicesTiles>().menu.is_some(),
        Kind::Workbench=>app.world().resource::<crate::tiles::recipe_menu::RecipeTiles>().menu.is_some()};
    panic!("actual painted service target missing: kind={:?} native={native} guard={g:?} frame_size={:?} density={} ui_scale={}",
        g.kind(),f.size,f.density,f.ui_scale);
}
// A current model refuses the previous native paint until its original writer runs.
// Only the two explicit external-change/transaction boundaries below use this witness.
fn publish_vendor_native(app:&mut App,wanted:impl Fn(Action)->bool) {
    if app.world().resource::<crate::tiles::repair_services_menu::RepairServicesTiles>().menu.is_none(){return}
    let before=guard(app);assert_eq!(before.kind(),Kind::Vendor);
    assert!(find_hit_point(app,&wanted).is_none(),"changed vendor model refuses preceding native paint");
    app.update();
    assert!(before.same(&guard(app)),"native publication alone preserves the current vendor model");
    assert!(find_hit_point(app,wanted).is_some(),"one original native writer publishes the current vendor target");
}
#[track_caller]
fn tap(app:&mut App,id:u64,wanted:impl Fn(Action)->bool) {
    let p=hit_point(app,wanted);edge(app,id,TouchPhase::Started,p);app.update();
    edge(app,id,TouchPhase::Ended,p);app.update();
}
#[track_caller]
fn row(app:&mut App,id:u64,index:usize) {
    let before=guard(app);let selected=|g:&Guard|match g {Guard::Vendor {view,..}=>view.selected,Guard::Workbench {view,..}=>view.selected};
    let native=match before.kind() {Kind::Vendor=>app.world().resource::<crate::tiles::repair_services_menu::RepairServicesTiles>().menu.is_some(),
        Kind::Workbench=>app.world().resource::<crate::tiles::recipe_menu::RecipeTiles>().menu.is_some()};
    let p=hit_point(app,|a|a==Action::Row(index));
    edge(app,id,TouchPhase::Started,p);app.update();edge(app,id,TouchPhase::Ended,p);app.update();
    if native && selected(&before)!=selected(&guard(app)) {
        // The original native writer precedes input. Selection changes after it,
        // so the new model cannot yet admit hits against the preceding paint.
        let f=app.world().resource::<Owner>().frame;let current=guard(app);
        let before=app.world_mut().run_system_once({let current=current.clone();move |geometry:Geometry,script:Res<ScriptState>| {
            geometry.hit(Some(&script.rt.as_ref().unwrap().game),f,&current,p)
        }}).unwrap();
        assert!(before.is_none(),"new selected model refuses the preceding native paint before its actual writer");
        app.update();
        let current=guard(app);
        let after=app.world_mut().run_system_once(move |geometry:Geometry,script:Res<ScriptState>| {
            geometry.hit(Some(&script.rt.as_ref().unwrap().game),f,&current,p)
        }).unwrap().expect("one actual native writer frame publishes the selected row");
        assert!(matches!(after.source,Source::Tile(..)));assert_eq!(after.action,Action::Row(index));
    }
}
fn quantity(app:&App)->Option<(i32,i32)> {let Guard::Workbench {view,..}=guard(app)else{panic!("workbench")};view.normal_quantity()}

#[test]
fn normal_services_native_and_handbuilt_drag_never_selects_or_spends() {
    let _units=crate::interface::units_at(1.0);
    for kind in [Kind::Vendor,Kind::Workbench] {for native in [false,true] {
        let mut app=scene(kind,native);let before=guard(&app);let p=hit_point(&mut app,|a|matches!(a,Action::Row(_)));
        edge(&mut app,1,TouchPhase::Started,p);app.update();edge(&mut app,1,TouchPhase::Moved,p-Vec2::Y*90.0);app.update();
        edge(&mut app,1,TouchPhase::Ended,p-Vec2::Y*90.0);app.update();app.update();
        let after=guard(&app);assert!(before.same(&after),"browse never changes selected row, quote, inventory or quantity");
        match after {Guard::Vendor {view,..}=>if native {
            assert!(app.world().resource::<crate::tiles::repair_services_menu::RepairServicesTiles>().value()>0.0);
        }else{assert!(view.first>0);},Guard::Workbench {view,..}=>if native {
            assert!(app.world().resource::<crate::tiles::recipe_menu::RecipeTiles>().value(0)>0.0);
        }else{
            let mut rows=app.world_mut().query::<&crate::crafting::RecipeButton>();
            let first=rows.iter(app.world()).filter_map(|b|if let crate::crafting::RecipeButton::Row(i)=b {Some(*i)}else{None}).min().unwrap();
            assert!(first>0,"actual measured handbuilt painter browsed past the first recipe");
            assert!(view.normal_quantity().is_none());
        }}
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,PLAYER,INPUT),12);assert_eq!(count(&app,PLAYER,OUTPUT),0);
    }}
}
#[test]
fn normal_vendor_real_end_repair_charges_once_and_stale_quote_refuses_then_recovers() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {
        let mut app=scene(Kind::Vendor,native);row(&mut app,1,1);
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert_eq!(view.selected,1);
        assert_eq!(count(&app,PLAYER,CAPS),10_000,"another-row End selects without repairing");
        let r=view.rows[view.selected].clone();let cost=r.quote.cost().unwrap();
        let p=hit_point(&mut app,|a|a==Action::Row(view.selected));
        edge(&mut app,2,TouchPhase::Started,p);app.update();assert_eq!(count(&app,PLAYER,CAPS),10_000);
        edge(&mut app,2,TouchPhase::Ended,p);edge(&mut app,2,TouchPhase::Ended,p);app.update();app.update();
        assert_eq!(count(&app,PLAYER,CAPS),10_000-cost as i32);assert_eq!(count(&app,VENDOR,CAPS),cost as i32);
        assert!(app.world().resource::<SimState>().world.peek(PLAYER).unwrap().inventory.as_ref().unwrap().groups()
            .any(|(id,unit,n)|id==r.item && n==1 && unit.condition_or_whole()>r.condition));
        let mut app=scene(Kind::Vendor,native);let p=hit_point(&mut app,|a|a==Action::Row(0));
        edge(&mut app,3,TouchPhase::Started,p);app.update();
        assert_eq!(app.world_mut().resource_mut::<SimState>().world.inventory_mut(PLAYER).unwrap().remove(CAPS,9_999),9_999);
        edge(&mut app,3,TouchPhase::Ended,p);app.update();app.update();assert_eq!(count(&app,PLAYER,CAPS),1);assert_eq!(count(&app,VENDOR,CAPS),0);
        app.world_mut().resource_mut::<SimState>().world.add_item(PLAYER,CAPS,9_999).unwrap();app.update();
        publish_vendor_native(&mut app,|a|a==Action::Row(0));
        row(&mut app,4,0);assert!(count(&app,VENDOR,CAPS)>0,"fresh actual quote remains usable");
    }
}
#[test]
fn normal_workbench_real_quantity_step_confirm_and_cancel_preserve_original_effects() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {
        let mut app=scene(Kind::Workbench,native);row(&mut app,9,1);
        let Guard::Workbench {view,..}=guard(&app)else{panic!("workbench")};assert_eq!(view.selected,1);
        assert_eq!(quantity(&app),None,"another-row End selects without opening quantity or making anything");
        row(&mut app,10,1);
        assert_eq!(quantity(&app),Some((1,6)),"original consumer always opens at one");
        assert_eq!(count(&app,PLAYER,INPUT),12);assert_eq!(count(&app,PLAYER,OUTPUT),0);
        tap(&mut app,11,|a|a==Action::Amount(Command::AmountStep(1)));assert_eq!(quantity(&app),Some((2,6)));
        // No extra settle frame between amount change and the next real raw Start.
        let p=hit_point(&mut app,|a|a==Action::Amount(Command::AmountOk));
        edge(&mut app,12,TouchPhase::Started,p);app.update();edge(&mut app,12,TouchPhase::Ended,p);edge(&mut app,12,TouchPhase::Ended,p);app.update();
        assert_eq!(count(&app,PLAYER,INPUT),8);assert_eq!(count(&app,PLAYER,OUTPUT),2);assert_eq!(quantity(&app),None);
        app.update();row(&mut app,13,1);assert_eq!(quantity(&app),Some((1,4)));
        tap(&mut app,14,|a|a==Action::Amount(Command::AmountCancel));assert_eq!(quantity(&app),None);
        assert_eq!(count(&app,PLAYER,INPUT),8);assert_eq!(count(&app,PLAYER,OUTPUT),2);
    }
}
#[test]
fn normal_services_registered_size_roundtrip_cancels_but_same_effective_value_preserves() {
    let _units=crate::interface::units_at(1.0);
    for kind in [Kind::Vendor,Kind::Workbench] {for native in [false,true] {
        let mut app=scene(kind,native);let key=match kind {Kind::Vendor=>"repair-menu-size-v1",Kind::Workbench=>"workbench-menu-size-v1"};
        let index=app.world().resource::<crate::settings::Settings>().index(key).unwrap();let p=hit_point(&mut app,|a|a==Action::Row(0));
        let default_part = crate::settings::choice_part(10, 31);
        let alt_part = crate::settings::choice_part(24, 31);
        edge(&mut app,20,TouchPhase::Started,p);app.update();
        assert_eq!(crate::settings::set_slider(app.world_mut(),index,alt_part),Some(alt_part));
        assert_eq!(crate::settings::set_slider(app.world_mut(),index,default_part),Some(default_part));
        edge(&mut app,20,TouchPhase::Ended,p);app.update();app.update();
        assert_eq!(count(&app,VENDOR,CAPS),0);if kind==Kind::Workbench{assert_eq!(quantity(&app),None);}
        let p=hit_point(&mut app,|a|a==Action::Row(0));edge(&mut app,21,TouchPhase::Started,p);app.update();
        assert_eq!(crate::settings::set_slider(app.world_mut(),index,default_part),Some(default_part));
        edge(&mut app,21,TouchPhase::Ended,p);app.update();app.update();
        match kind {Kind::Vendor=>assert!(count(&app,VENDOR,CAPS)>0),Kind::Workbench=>assert_eq!(quantity(&app),Some((1,6)))}
    }}
}

#[test]
fn normal_vendor_repair_all_confirmation_and_done_keep_original_consumers() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {
        let mut app=scene(Kind::Vendor,native);
        app.insert_resource(crate::tweaks::TweakSettings {repair_all_confirm:true,repair_all_threshold:0.0,..default()});
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};let total=view.total;
        assert!(view.all_ready && total>0);
        tap(&mut app,30,|a|a==Action::Vendor(crate::repair::RepairServicesInput::All));
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert!(view.confirming);
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,VENDOR,CAPS),0);
        for _ in 0..3 {app.update();}
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert!(view.confirming,"idle frames preserve the pending deliberate confirmation");
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,VENDOR,CAPS),0);
        tap(&mut app,31,|a|a==Action::Vendor(crate::repair::RepairServicesInput::All));
        assert_eq!(count(&app,PLAYER,CAPS),10_000-total as i32);assert_eq!(count(&app,VENDOR,CAPS),total as i32);
        let paid=count(&app,PLAYER,CAPS);
        publish_vendor_native(&mut app,|a|a==Action::Vendor(crate::repair::RepairServicesInput::Done));
        tap(&mut app,32,|a|a==Action::Vendor(crate::repair::RepairServicesInput::Done));app.update();
        assert!(!app.world().resource::<crate::repair::RepairServices>().is_open());assert_eq!(count(&app,PLAYER,CAPS),paid);
        assert_eq!(*app.world().resource::<State<crate::PlayState>>().get(),crate::PlayState::Playing);
    }
}

#[derive(Resource,Default)]
struct Late(Option<bool>);
fn late_owner(world:&mut World) {
    let Some(creator)=world.resource_mut::<Late>().0.take() else{return};
    let code=match world.resource::<Owner>().kind {Kind::Vendor=>fnv_game::repair::REPAIR_SERVICES_MENU,
        Kind::Workbench=>fnv_game::crafting::RECIPE_MENU};
    world.resource_mut::<ScriptState>().rt.as_mut().unwrap().game.open_menu(if creator {
        fnv_game::game::menus::RACE
    }else{fnv_game::game::menus::MESSAGE});
    if creator {
        crate::creator::synchronize_for_map_control(world);
        assert!(world.resource::<crate::creator::CreatorMenu>().is_open(),"real creator sync created the higher owner");
        // Deliberately restore the script code before input; the actual creator
        // model remains open until its next sync and must still refuse this tap.
        world.resource_mut::<ScriptState>().rt.as_mut().unwrap().game.menu=Some(code);
    }
}
#[test]
fn normal_services_late_message_or_actual_creator_after_dispatch_refuses_and_fresh_owner_recovers() {
    let _units=crate::interface::units_at(1.0);
    for kind in [Kind::Vendor,Kind::Workbench] {for native in [false,true] {for creator in [false,true] {
        let mut app=scene(kind,native);
        let data=fnv_actor::creator::Creator {player:Some(fnv_actor::Look {race:FormId(0x19),female:false,
            hair:None,hair_color:None,eyes:None,head_parts:vec![],facegen:default()}),..default()};
        app.insert_resource(crate::creator::CreatorData(Arc::new(data)))
            .init_resource::<crate::creator::CreatorMenu>().init_resource::<crate::control::PlayerControl>()
            .init_resource::<Late>().add_systems(Update,late_owner.after(Dispatch)
                .before(crate::tiles::input::feed).before(crate::repair::NormalInput).before(crate::crafting::NormalInput));
        let p=hit_point(&mut app,|a|a==Action::Row(0));edge(&mut app,40,TouchPhase::Started,p);app.update();
        app.world_mut().resource_mut::<Late>().0=Some(creator);
        edge(&mut app,40,TouchPhase::Ended,p);app.update();
        assert_eq!(count(&app,VENDOR,CAPS),0);assert_eq!(count(&app,PLAYER,OUTPUT),0);
        if kind==Kind::Workbench{assert_eq!(quantity(&app),None);}
        {
            let mut script=app.world_mut().resource_mut::<ScriptState>();let game=&mut script.rt.as_mut().unwrap().game;
            game.close_menu();game.menu_under=None;game.open_menu(match kind {Kind::Vendor=>fnv_game::repair::REPAIR_SERVICES_MENU,
                Kind::Workbench=>fnv_game::crafting::RECIPE_MENU});
        }
        crate::creator::synchronize_for_map_control(app.world_mut());
        assert!(!app.world().resource::<crate::creator::CreatorMenu>().is_open());app.update();
        row(&mut app,41,0);
        match kind {Kind::Vendor=>assert!(count(&app,VENDOR,CAPS)>0),Kind::Workbench=>assert_eq!(quantity(&app),Some((1,6)))}
    }}}
}
#[test]
fn normal_services_saved_off_on_cancel_secondary_and_foreign_window_tails_need_fresh_owner() {
    let _units=crate::interface::units_at(1.0);
    for kind in [Kind::Vendor,Kind::Workbench] {for native in [false,true] {
        let mut app=scene(kind,native);let setting=app.world().resource::<crate::settings::Settings>()
            .index(crate::maintenance_touch::SETTING_KEY).unwrap();
        let p=hit_point(&mut app,|a|a==Action::Row(0));edge(&mut app,50,TouchPhase::Started,p);app.update();
        assert_eq!(crate::settings::flip(app.world_mut(),setting),Some(false));
        assert_eq!(crate::settings::flip(app.world_mut(),setting),Some(true));
        edge(&mut app,50,TouchPhase::Ended,p);app.update();app.update();
        assert_eq!(count(&app,VENDOR,CAPS),0);if kind==Kind::Workbench{assert_eq!(quantity(&app),None);}
        edge(&mut app,51,TouchPhase::Started,p);app.update();edge(&mut app,52,TouchPhase::Started,p);app.update();
        edge(&mut app,52,TouchPhase::Ended,p);app.update();
        assert!(app.world().resource::<NormalServices>().contact.as_ref().is_some_and(|c|c.id==51),"unrelated End cannot take owning contact");
        edge(&mut app,51,TouchPhase::Canceled,p);app.update();app.update();
        let other=app.world_mut().spawn(Window::default()).id();
        for phase in [TouchPhase::Started,TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {window:other,id:53,phase,position:p,force:None});app.update();
        }
        assert_eq!(count(&app,VENDOR,CAPS),0);if kind==Kind::Workbench{assert_eq!(quantity(&app),None);}
        app.update();row(&mut app,54,0);
        match kind {Kind::Vendor=>assert!(count(&app,VENDOR,CAPS)>0),Kind::Workbench=>assert_eq!(quantity(&app),Some((1,6)))}
    }}
}
fn actual_rect(world:&World,e:Entity)->Rect {
    let n=world.get::<ComputedNode>(e).expect("actual Taffy layout");
    Rect::from_center_size(world.get::<UiGlobalTransform>(e).unwrap().translation,n.size())
}
#[test]
fn normal_vendor_actual_condition_bars_and_equipped_marker_fit_adjacent_rows_at_saved_sizes() {
    let _units=crate::interface::units_at(1.0);
    for size in [0.5,1.0,1.5] {
        let mut app=scene(Kind::Vendor,false);let index=app.world().resource::<crate::settings::Settings>()
            .index("repair-menu-size-v1").unwrap();let part=size-0.5;
        assert_eq!(crate::settings::set_slider(app.world_mut(),index,part),Some(part));app.update();app.update();
        let mut q=app.world_mut().query::<(Entity,&crate::repair::RepairButton)>();
        let mut rows:Vec<_>=q.iter(app.world()).filter_map(|(e,b)|if let crate::repair::RepairButton::Row(i)=b {Some((*i,e))}else{None}).collect();
        rows.sort_by_key(|(i,_)|*i);assert!(rows.len()>1);
        let mut bars=0;let mut markers=0;
        for (_,e) in &rows {
            let row=actual_rect(app.world(),*e);
            let node=app.world().get::<Node>(*e).unwrap();let computed=app.world().get::<ComputedNode>(*e).unwrap();
            let frame=app.world().resource::<Owner>().frame;let height=frame.size.y*frame.density;
            let minimum=51.0*height/960.0;
            let Val::Vh(declared)=node.height else{panic!("actual repair row must declare interface viewport units")};
            assert!(declared*height/100.0>=minimum-0.1,"51-unit declaration at size={size}: {declared}vh minimum={minimum}");
            assert!(computed.unrounded_size().y>=minimum-0.1,"51-unit unrounded layout at size={size}: {:?} minimum={minimum}",computed.unrounded_size());
            assert!(row.height()>=minimum.floor(),"rounded physical row at size={size}: {row:?} minimum={minimum}");
            assert!((row.height()-computed.unrounded_size().y).abs()<=1.0,"at most one physical pixel of actual Bevy rounding: {row:?}, {:?}",computed.unrounded_size());
            if let Some(children)=app.world().get::<Children>(*e) {for child in children {
                let n=app.world().get::<Node>(*child).unwrap();
                if let (Val::Vh(w),Val::Vh(h))=(n.width,n.height) {
                    if h<=0.0 || w<=0.0 {continue}
                    let r=actual_rect(app.world(),*child);
                    assert!(r.min.y>=row.min.y-0.1 && r.max.y<=row.max.y+0.1,"actual decoration must remain in its measured row at {size}");
                    if (h-16.0/9.6).abs()<0.01 {bars+=1;}
                    if (h-25.0/9.6).abs()<0.01 && (w-25.0/9.6).abs()<0.01 {markers+=1;}
                }
            }}
        }
        assert!(bars>=rows.len()*2,"actual condition backing and filled bar per row");assert_eq!(markers,1,"actual World equip produces exactly one marker");
        // Absolute siblings resolve fractional positions/sizes independently in Taffy.
        // Preserve exact unrounded adjacency and reconcile only its actual pixel rounding.
        for pair in rows.windows(2) {
            let (a,b)=(actual_rect(app.world(),pair[0].1),actual_rect(app.world(),pair[1].1));
            assert_eq!(app.world().get::<ChildOf>(pair[0].1).unwrap().parent(),
                app.world().get::<ChildOf>(pair[1].1).unwrap().parent(),"adjacent repair rows share the same actual parent");
            let (left,right,rounded_left,rounded_right)={
                let mut surface=app.world_mut().resource_mut::<UiSurface>();
                (surface.get_layout(pair[0].1,false).unwrap().0,surface.get_layout(pair[1].1,false).unwrap().0,
                    surface.get_layout(pair[0].1,true).unwrap().0,surface.get_layout(pair[1].1,true).unwrap().0)
            };
            let intended_delta=left.location.y+left.size.height-right.location.y;
            let rounded_delta=rounded_left.location.y+rounded_left.size.height-rounded_right.location.y;
            let actual_delta=a.max.y-b.min.y;
            assert!(intended_delta.abs()<=0.1,
                "unrounded adjacent rows at size={size}, indices={:?}: rects={a:?}/{b:?}, layouts={left:?}/{right:?}", (pair[0].0,pair[1].0));
            assert!((actual_delta-rounded_delta).abs()<=0.1,
                "actual row edges reconcile to Taffy rounding at size={size}, indices={:?}: rects={a:?}/{b:?}, rounded={rounded_left:?}/{rounded_right:?}", (pair[0].0,pair[1].0));
            assert!(actual_delta<=1.0,
                "at most one physical pixel of independently rounded adjacency at size={size}, indices={:?}: rects={a:?}/{b:?}, unrounded={left:?}/{right:?}, rounded={rounded_left:?}/{rounded_right:?}", (pair[0].0,pair[1].0));
        }
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,VENDOR,CAPS),0);
    }
}

fn physical_key(app:&mut App,code:KeyCode,key:bevy::input::keyboard::Key,down:bool) {
    use bevy::input::{keyboard::KeyboardInput,ButtonState};
    let window=app.world().resource::<Owner>().frame.window;
    app.world_mut().write_message(KeyboardInput {key_code:code,logical_key:key,
        state:if down {ButtonState::Pressed}else{ButtonState::Released},text:None,repeat:false,window});
    app.update();
}

#[test]
fn normal_workbench_original_physical_navigation_follows_last_row_in_measured_native_and_handbuilt_views() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {for size in [0.5,1.0,1.5] {
        let mut app=scene(Kind::Workbench,native);let index=app.world().resource::<crate::settings::Settings>()
            .index("workbench-menu-size-v1").unwrap();assert_eq!(crate::settings::set_slider(app.world_mut(),index,size-0.5),Some(size-0.5));
        app.update();app.update();
        for _ in 0..11 {
            physical_key(&mut app,KeyCode::ArrowDown,bevy::input::keyboard::Key::ArrowDown,true);
            physical_key(&mut app,KeyCode::ArrowDown,bevy::input::keyboard::Key::ArrowDown,false);
        }
        let Guard::Workbench {view,..}=guard(&app)else{panic!("workbench")};assert_eq!(view.selected,11);
        let p=hit_point(&mut app,|a|a==Action::Row(11));let frame=app.world().resource::<Owner>().frame;
        assert!(Rect::from_corners(Vec2::ZERO,frame.size).contains(p),"original key selection followed into current measured viewport");
        assert_eq!(count(&app,PLAYER,INPUT),12);assert_eq!(count(&app,PLAYER,OUTPUT),0);
        physical_key(&mut app,KeyCode::Enter,bevy::input::keyboard::Key::Enter,true);
        physical_key(&mut app,KeyCode::Enter,bevy::input::keyboard::Key::Enter,false);
        assert_eq!(quantity(&app),Some((1,6)),"physical confirmation retains original quantity defaults");
        physical_key(&mut app,KeyCode::Escape,bevy::input::keyboard::Key::Escape,true);
        physical_key(&mut app,KeyCode::Escape,bevy::input::keyboard::Key::Escape,false);
        assert_eq!(quantity(&app),None);assert_eq!(count(&app,PLAYER,INPUT),12);assert_eq!(count(&app,PLAYER,OUTPUT),0);
    }}
}

#[test]
fn normal_services_native_entry_waits_for_original_writer_publication_then_accepts_fresh_touch() {
    let _units=crate::interface::units_at(1.0);
    for kind in [Kind::Vendor,Kind::Workbench] {
        let mut app=unsettled_scene(kind,true);app.update();
        // The real request opened its owner in Update. No prior painter/writer
        // source exists yet when the next frame's raw event is admitted.
        let published=match kind {Kind::Vendor=>app.world().resource::<crate::repair::RepairServices>().normal_guard(),
            Kind::Workbench=>app.world().resource::<crate::crafting::RecipeMenu>().normal_guard()};
        assert!(published.is_none());
        edge(&mut app,60,TouchPhase::Started,Vec2::new(80.0,110.0));app.update();
        assert!(app.world().resource::<NormalServices>().contact.is_none(),"unpublished native content cannot author a contact");
        edge(&mut app,60,TouchPhase::Ended,Vec2::new(80.0,110.0));app.update();
        for _ in 0..4 {app.update();}
        assert_eq!(count(&app,VENDOR,CAPS),0);assert_eq!(count(&app,PLAYER,OUTPUT),0);
        if kind==Kind::Workbench{assert_eq!(quantity(&app),None);}
        row(&mut app,61,0);
        match kind {Kind::Vendor=>assert!(count(&app,VENDOR,CAPS)>0),Kind::Workbench=>assert_eq!(quantity(&app),Some((1,6)))}
    }
}

#[test]
fn normal_workbench_late_ingredient_loss_refuses_confirm_then_fresh_real_stock_recovers() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {
        let mut app=scene(Kind::Workbench,native);row(&mut app,70,0);assert_eq!(quantity(&app),Some((1,6)));
        let p=hit_point(&mut app,|a|a==Action::Amount(Command::AmountOk));
        edge(&mut app,71,TouchPhase::Started,p);app.update();
        assert_eq!(app.world_mut().resource_mut::<SimState>().world.inventory_mut(PLAYER).unwrap().remove(INPUT,12),12);
        edge(&mut app,71,TouchPhase::Ended,p);app.update();
        assert_eq!(count(&app,PLAYER,INPUT),0);assert_eq!(count(&app,PLAYER,OUTPUT),0);
        assert_eq!(quantity(&app),Some((1,6)),"stale proposal did not reach make or dismiss the original prompt");
        app.world_mut().resource_mut::<SimState>().world.add_item(PLAYER,INPUT,12).unwrap();app.update();
        tap(&mut app,72,|a|a==Action::Amount(Command::AmountOk));
        assert_eq!(count(&app,PLAYER,INPUT),10);assert_eq!(count(&app,PLAYER,OUTPUT),1);assert_eq!(quantity(&app),None);
    }
}
#[test]
fn normal_services_bounded_secondary_overflow_retains_release_mask_then_real_neutral_recovers() {
    let _units=crate::interface::units_at(1.0);
    for kind in [Kind::Vendor,Kind::Workbench] {
        let mut app=scene(kind,false);let p=hit_point(&mut app,|a|a==Action::Row(0));
        for id in 100..135 {edge(&mut app,id,TouchPhase::Started,p);}app.update();
        let owner=app.world().resource::<Owner>();let state=app.world().resource::<NormalServices>();
        assert_eq!(state.overflow,Some(owner.frame.window));assert!(state.retired.len()<=MAX_IDS);assert!(state.contact.is_none());
        assert!((100..135).all(|id|state.captures_touch(owner.frame.window,id)));
        for id in 100..135 {edge(&mut app,id,TouchPhase::Ended,p);}app.update();
        let frame=app.world().resource::<Owner>().frame;
        assert!(app.world().resource::<NormalServices>().captures_touch(frame.window,134),"release frame remains masked");
        assert_eq!(count(&app,VENDOR,CAPS),0);assert_eq!(count(&app,PLAYER,OUTPUT),0);
        app.update();assert!(app.world().resource::<NormalServices>().overflow.is_none());
        assert!(app.world().resource::<NormalServices>().retired.is_empty(),"only actual no-held/no-release neutral clears quarantine");
        row(&mut app,150,0);
        match kind {Kind::Vendor=>assert!(count(&app,VENDOR,CAPS)>0),Kind::Workbench=>assert_eq!(quantity(&app),Some((1,6)))}
    }
}

// Queue an ordinary native command after touch dispatch, before the same original
// repair consumer. This is a competing input, not a private effect/state writer.
#[derive(Resource,Default)]
struct LateSelection(bool);
fn late_selection(mut late:ResMut<LateSelection>,mut commands:MessageWriter<crate::repair::RepairServicesInput>) {
    if std::mem::take(&mut late.0) {commands.write(crate::repair::RepairServicesInput::Select(1));}
}
#[test]
fn normal_vendor_late_selection_before_native_feed_cannot_be_reset_by_owned_repair_click() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {
        let mut app=scene(Kind::Vendor,native);
        app.init_resource::<LateSelection>().add_systems(Update,late_selection.after(Dispatch)
            .before(crate::tiles::input::feed).before(crate::repair::NormalInput));
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert_eq!(view.selected,0);
        let p=hit_point(&mut app,|a|a==Action::Row(0));
        edge(&mut app,90,TouchPhase::Started,p);app.update();
        app.world_mut().resource_mut::<LateSelection>().0=true;
        edge(&mut app,90,TouchPhase::Ended,p);app.update();
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};
        assert_eq!(view.selected,1,"the actual newer Select must survive the stale touch envelope");
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,VENDOR,CAPS),0);
        app.update();row(&mut app,91,1);
        assert!(count(&app,VENDOR,CAPS)>0,"fresh selected-row contact uses the original repair effect");
    }
}

#[test]
fn normal_vendor_original_keyboard_and_native_requests_keep_confirmation_until_real_other_intent() {
    let _units=crate::interface::units_at(1.0);
    for native in [false,true] {
        let mut app=scene(Kind::Vendor,native);
        app.insert_resource(crate::tweaks::TweakSettings {repair_all_confirm:true,repair_all_threshold:0.0,..default()});
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};let total=view.total;
        assert!(view.all_ready && total>0);
        app.world_mut().write_message(crate::repair::RepairServicesInput::All);app.update();
        for _ in 0..3 {app.update();}
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert!(view.confirming);
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,VENDOR,CAPS),0);
        // The original native selection consumer is a genuine other intent.
        app.world_mut().write_message(crate::repair::RepairServicesInput::Select(1));app.update();
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert_eq!(view.selected,1);assert!(!view.confirming);
        physical_key(&mut app,KeyCode::KeyA,bevy::input::keyboard::Key::Character("a".into()),true);
        physical_key(&mut app,KeyCode::KeyA,bevy::input::keyboard::Key::Character("a".into()),false);
        let Guard::Vendor {view,..}=guard(&app)else{panic!("vendor")};assert!(view.confirming,"the first actual keyboard All asks again after selection");
        assert_eq!(count(&app,PLAYER,CAPS),10_000);assert_eq!(count(&app,VENDOR,CAPS),0);
        app.world_mut().write_message(crate::repair::RepairServicesInput::All);app.update();
        assert_eq!(count(&app,PLAYER,CAPS),10_000-total as i32);assert_eq!(count(&app,VENDOR,CAPS),total as i32);
        let paid=count(&app,PLAYER,CAPS);for _ in 0..3 {app.update();}
        assert_eq!(count(&app,PLAYER,CAPS),paid,"idle cannot repeat the completed repair");
        app.world_mut().write_message(crate::repair::RepairServicesInput::Done);app.update();app.update();
        assert!(!app.world().resource::<crate::repair::RepairServices>().is_open());
        assert_eq!(count(&app,PLAYER,CAPS),paid);
    }
}

#[test]
fn normal_workbench_recipes_header_horizontal_swipe_switches_filter() {
    let _units = crate::interface::units_at(1.0);
    // Use hand-built workbench with real subcategories so horizontal swipe on the
    // RECIPES header exercises filter cycling end-to-end.
    let mut app = scene(Kind::Workbench, false);
    let sub_a = FormId(0xFC131);
    let sub_b = FormId(0xFC132);
    {
        let refs = app.world().resource::<SimState>().refs;
        let schemas = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let functions = schemas.join("script/functions.json");
        let json = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
        let mut owned = fnv_game::data::GameData::build(
            &fnv_esm::LoadOrder::new(&[]).unwrap(),
            &fnv_esm::Schema::load_dir(schemas.join("fnv")).unwrap(),
            fnv_script::FunctionTable::load(&functions).unwrap(),
            &json,
            refs,
        );
        owned.recipe_categories.insert(CATEGORY, fnv_game::crafting::RecipeCategoryDef { name: "Authored workbench".into(), subcategory: false });
        owned.recipe_categories.insert(sub_a, fnv_game::crafting::RecipeCategoryDef { name: "Ammo".into(), subcategory: true });
        owned.recipe_categories.insert(sub_b, fnv_game::crafting::RecipeCategoryDef { name: "Aid".into(), subcategory: true });
        for i in 0..12 {
            let form = FormId(0xFC200 + i);
            owned.recipes.insert(form, fnv_game::crafting::RecipeDef {
                form,
                name: format!("Workbench recipe {i:02}"),
                category: Some(CATEGORY),
                subcategory: Some(if i % 2 == 0 { sub_a } else { sub_b }),
                ingredients: vec![(INPUT, 2)],
                outputs: vec![(OUTPUT, 1)],
                ..default()
            });
        }
        let mut script = app.world_mut().resource_mut::<ScriptState>();
        script.rt.as_mut().unwrap().game.data = Box::leak(Box::new(owned));
    }
    for _ in 0..3 { app.update(); }
    let Guard::Workbench { filter: initial, view, .. } = guard(&app) else { panic!("workbench") };
    assert_eq!(view.filters, 2, "fixture exposes two recipe subcategories");
    let p = hit_point(&mut app, |a| a == Action::CategoryHeader);
    // Swipe right -> next subcategory (+1)
    edge(&mut app, 410, TouchPhase::Started, p); app.update();
    edge(&mut app, 410, TouchPhase::Moved, p + Vec2::new(40.0, 0.0)); app.update();
    edge(&mut app, 410, TouchPhase::Ended, p + Vec2::new(45.0, 0.0)); app.update(); app.update();
    let Guard::Workbench { filter: after_right, .. } = guard(&app) else { panic!("workbench") };
    assert_ne!(after_right, initial, "right swipe on Recipes header advances subcategory filter");
    // Swipe left -> back (-1)
    let p2 = hit_point(&mut app, |a| a == Action::CategoryHeader);
    edge(&mut app, 411, TouchPhase::Started, p2); app.update();
    edge(&mut app, 411, TouchPhase::Moved, p2 - Vec2::new(40.0, 0.0)); app.update();
    edge(&mut app, 411, TouchPhase::Ended, p2 - Vec2::new(45.0, 0.0)); app.update(); app.update();
    let Guard::Workbench { filter: after_left, .. } = guard(&app) else { panic!("workbench") };
    assert_eq!(after_left, initial, "left swipe on Recipes header reverses subcategory filter");
}
