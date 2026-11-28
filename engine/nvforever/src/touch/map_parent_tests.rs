// Tests the sole wrapper reader, not a second test Bridge or direct core event call.
use nvf_play::pipboy::{self as map_pip, data::{self as map_data, touch_map::TouchMap, travel::{FastTravelAsk,FastTravelGo}},touch_items::TouchItems};

#[derive(Resource,Default)]
struct MapParentSeen {asks:Vec<u32>,go:Vec<u32>}
fn observe_map_parent(mut ask:MessageReader<FastTravelAsk>,mut go:MessageReader<FastTravelGo>,mut seen:ResMut<MapParentSeen>) {
    seen.asks.extend(ask.read().map(|m|m.0));seen.go.extend(go.read().map(|m|m.0));
}
fn map_parent_scene(enabled:bool)->(App,Entity) {
    use fnv_esm::raw::{write_subrecords,Subrecord};
    fn sub(sig:&[u8;4],data:Vec<u8>)->Subrecord {Subrecord{sig:*sig,data,extended:false}}
    fn record(sig:&[u8;4],id:u32,subs:&[Subrecord])->Vec<u8> {
        let payload=write_subrecords(subs);
        [&sig[..],&(payload.len() as u32).to_le_bytes(),&[0;4],&id.to_le_bytes(),&[0;4],&15u16.to_le_bytes(),&[0;2],&payload].concat()
    }
    fn group(id:u32,kind:u32,children:Vec<u8>)->Vec<u8> {
        [b"GRUP".as_slice(),&((24+children.len()) as u32).to_le_bytes(),&id.to_le_bytes(),&kind.to_le_bytes(),&[0;8],&children].concat()
    }
    let (mut app,window)=default_menu_touch_scene(map_pip::Page::Data,enabled);
    // Original, synthetic records use the real schema/WRLD/map-marker reader.
    let hedr=[1.34f32.to_le_bytes(),0i32.to_le_bytes(),0x10000u32.to_le_bytes()].concat();
    let mnam=[2048i32.to_le_bytes().to_vec(),2048i32.to_le_bytes().to_vec(),[-2i16,3,1,2].into_iter().flat_map(i16::to_le_bytes).collect()].concat();
    let reference=record(b"REFR",0xF811,&[sub(b"NAME",0x10u32.to_le_bytes().to_vec()),sub(b"DATA",[0f32,12288.,0.,0.,0.,0.].into_iter().flat_map(f32::to_le_bytes).collect()),sub(b"XMRK",vec![]),sub(b"FULL",b"Authored map destination\0".to_vec()),sub(b"FNAM",vec![3]),sub(b"TNAM",vec![1])]);
    let cell=record(b"CELL",0xF810,&[sub(b"DATA",vec![0]),sub(b"XCLC",[0i32.to_le_bytes(),3i32.to_le_bytes()].concat())]);
    let children=group(0xF800,1,group(0,4,group(0,5,[cell,group(0xF810,6,group(0xF810,9,reference))].concat())));
    let bytes=[record(b"TES4",0,&[sub(b"HEDR",hedr)]),
        record(b"WRLD",0xF800,&[sub(b"ICON",b"authored-map.dds\0".to_vec()),sub(b"MNAM",mnam)]),
        children].concat();
    let plugins=Box::leak(vec![fnv_esm::Plugin::parse(&bytes,"OwnedMap.esm".into()).unwrap()].into_boxed_slice());
    let lo=Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema=Box::leak(Box::new(fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap()));
    let refs=Box::leak(Box::new(fnv_sim::RefTable::build(lo,schema)));
    assert_eq!(refs.refs[&fnv_sim::FormId(0xF811)].cell,Some(fnv_sim::FormId(0xF810)));
    assert_eq!(refs.cell_world[&fnv_sim::FormId(0xF810)],fnv_sim::FormId(0xF800));
    let forms=app.world().resource::<nvf_play::sim::SimState>().forms;
    app.insert_resource(nvf_play::sim::SimState{forms,refs,world:fnv_sim::World::new(forms,refs,fnv_sim::SimConfig::default())});
    app.insert_resource(nvf_play::PlayWorld(nvf_play::PlayData {
        lo,schema,index:Box::leak(Box::new(fnv_world::WorldIndex::build(lo,schema))),world:0xF800,interior:None,
        eye:[0.,12288.,0.],heading_deg:0.,pitch_deg:0.,auto_walk:0.,start_at_cell_spot:false,packages:false,
    })).insert_resource(nvf_play::Location{world:0xF800,interior:None})
        .insert_resource(nvf_play::player::Player{heading:0.,pitch:0.,eye_height:0.,feet:[0.,12288.,0.],body_eye:None,body_view:None,placed:None})
        .init_resource::<map_data::DataState>().init_resource::<map_pip::frame::Kit>()
        .init_resource::<nvf_play::pad::PadGlyphs>().init_resource::<MapParentSeen>()
        // PlayPlugin normally gets this real DATA Input.menu stream from MenuPlugin.
        .add_message::<nvf_play::menu::MenuInput>()
        .add_message::<FastTravelAsk>().add_message::<FastTravelGo>()
        .add_message::<map_data::SummonCompanions>()
        .add_systems(Update,map_data::draw.after(map_pip::flat_screen))
        .add_systems(Update,observe_map_parent.after(map_data::draw));
    assert!(app.world().contains_resource::<bevy::ecs::message::Messages<nvf_play::menu::MenuInput>>(),
        "actual DATA draw requires MenuInput messages before its first system pass");
    app.world_mut().resource_mut::<map_pip::PipBoy>().tabs[map_pip::Page::Data.index()]=1;
    let camera=app.world().resource::<map_pip::PipBoyScreen>().camera;
    app.world_mut().spawn((Node::default(),UiTargetCamera(camera),map_pip::PageBody{page:map_pip::Page::Data,tab:1}));
    for _ in 0..6 {app.update();}
    assert!(!app.world().resource::<TouchMap>().captures());
    (app,window)
}
fn map_parent_raw(app:&mut App,canvas:Vec2)->Vec2 {
    let world=app.world_mut();
    let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<map_pip::FlatScreen>>();
    let (node,at)=q.single(world).unwrap();
    let physical=at.translation-node.size()*0.5+node.size()*canvas/map_pip::SCREEN_CROP;
    let mut q=world.query_filtered::<&Window,With<PrimaryWindow>>();
    physical/q.single(world).unwrap().scale_factor()
}
fn assert_map_parent_owner(app:&App,id:u64) {
    assert!(app.world().resource::<TouchMap>().captures(),"actual wrapper must admit the map before generic menu_start");
    assert!(!app.world().resource::<TouchItems>().owns(id),"the same raw contact cannot also enter item/tab projection");
    assert!(app.world().resource::<TouchState>().contacts.is_empty());
    assert!(app.world().resource::<TouchState>().ui_capture);
}
#[test]
fn sole_parent_raw_map_pan_and_pinch_precede_default_off_and_enabled_bar_generic_menu_ingress() {
    for enabled in [false,true] {for pinch in [false,true] {
        let (mut app,w)=map_parent_scene(enabled);
        let centre=map_parent_raw(&mut app,Vec2::new(477.5,350.));
        let a=centre-Vec2::X*40.;let b=centre+Vec2::X*40.;
        touch(&mut app,w,2100,TouchPhase::Started,a.x,a.y);app.update();assert_map_parent_owner(&app,2100);
        if pinch {touch(&mut app,w,2101,TouchPhase::Started,b.x,b.y);app.update();assert_map_parent_owner(&app,2101);}
        let moved=if pinch {a-Vec2::X*40.}else{a+Vec2::new(40.,20.)};
        touch(&mut app,w,2100,TouchPhase::Moved,moved.x,moved.y);app.update();assert_map_parent_owner(&app,2100);
        touch(&mut app,w,2100,TouchPhase::Ended,moved.x,moved.y);
        if pinch {touch(&mut app,w,2101,TouchPhase::Ended,b.x,b.y);}
        app.update();for _ in 0..5 {app.update();}
        let seen=app.world().resource::<MapParentSeen>();assert!(seen.asks.is_empty()&&seen.go.is_empty());
        let seen=app.world().resource::<DefaultMenuTouchSeen>();assert_eq!((seen.presses,seen.releases,seen.scrolls),(0,0,0));
        assert!(app.world().resource::<nvf_play::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
        assert!(!app.world().resource::<TouchMap>().captures());
    }}
}
#[test]
fn sole_parent_raw_marker_tap_uses_real_record_and_existing_travel_request_once_with_bar_off_and_on() {
    for enabled in [false,true] {
        let (mut app,w)=map_parent_scene(enabled);let p=map_parent_raw(&mut app,Vec2::new(477.5,350.));
        touch(&mut app,w,2200,TouchPhase::Started,p.x,p.y);app.update();assert_map_parent_owner(&app,2200);
        touch(&mut app,w,2200,TouchPhase::Ended,p.x,p.y);app.update();for _ in 0..5 {app.update();}
        assert_eq!(app.world().resource::<MapParentSeen>().asks,[0xF811]);
        assert!(app.world().resource::<MapParentSeen>().go.is_empty(),"touch only asks; legal travel still owns confirmation");
        let seen=app.world().resource::<DefaultMenuTouchSeen>();assert_eq!((seen.presses,seen.releases,seen.scrolls),(0,0,0));
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
    }
}

#[derive(Resource,Default)]
struct LateMapRenderTarget(Option<(bevy::camera::RenderTarget,Vec2)>);
fn change_map_render_target_after_raw(world:&mut World) {
    let Some((target,point))=world.resource_mut::<LateMapRenderTarget>().0.take() else {return;};
    assert!(world.resource::<TouchMap>().captures(),"the sole raw reader already admitted this terminal batch");
    assert!(world.resource::<TouchMap>().projected_point(point).is_some(),"the real PointerSet publisher accepted the primary target before this late mutation");
    let camera={let mut q=world.query_filtered::<Entity,With<nvf_play::PlayerCamera>>();q.single(world).unwrap()};
    world.entity_mut(camera).insert(target);
}
#[test]
fn actual_device_map_consumer_rejects_same_sized_target_switch_after_raw_end_but_accepts_same_window_alias() {
    use bevy::{camera::RenderTarget,window::WindowRef};
    for target_case in ["image","other-window","same-window"] {
        let (mut app,w)=map_parent_scene(false);
        crate::pipboy_arm::tests::attach_map_pointer_fixture(&mut app);
        app.init_resource::<LateMapRenderTarget>().add_systems(Update,change_map_render_target_after_raw.after(map_pip::PointerSet).before(map_data::draw));
        for _ in 0..5 {app.update();}
        let p=crate::pipboy_arm::tests::map_pointer_fixture_raw(&mut app,Vec2::new(477.5,350.));
        assert!((app.world().resource::<TouchMap>().projected_point(p).unwrap()-Vec2::new(477.5,350.)).length()<0.01,
            "use the real nearest-visible posed screen projection, never a synthetic mapper");
        touch(&mut app,w,2300,TouchPhase::Started,p.x,p.y);app.update();assert_map_parent_owner(&app,2300);
        let target=match target_case {
            "image"=>RenderTarget::Image(Handle::<Image>::default().into()),
            "other-window"=>{let other=app.world_mut().spawn(Window{resolution:bevy::window::WindowResolution::new(1000,1000),..default()}).id();RenderTarget::Window(WindowRef::Entity(other))},
            _=>RenderTarget::Window(WindowRef::Entity(w)),
        };
        app.world_mut().resource_mut::<LateMapRenderTarget>().0=Some((target,p));
        touch(&mut app,w,2300,TouchPhase::Ended,p.x,p.y);app.update();
        assert_eq!(app.world().resource::<MapParentSeen>().asks,
            if target_case=="same-window" {vec![0xF811]}else{vec![]},
            "late consumer admission must validate the actual render destination, not just camera dimensions");
        assert!(app.world().resource::<MapParentSeen>().go.is_empty());
        let camera={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<nvf_play::PlayerCamera>>();q.single(world).unwrap()};
        let info=app.world().get::<Camera>(camera).unwrap().computed.target_info.as_ref().unwrap();
        assert_eq!(info.physical_size,UVec2::splat(1000),"the rejection is target identity, not a dimension mismatch");
        app.world_mut().entity_mut(camera).insert(RenderTarget::Window(WindowRef::Primary));
        for _ in 0..5 {app.update();}
        touch(&mut app,w,2301,TouchPhase::Started,p.x,p.y);app.update();assert_map_parent_owner(&app,2301);
        touch(&mut app,w,2301,TouchPhase::Ended,p.x,p.y);app.update();
        assert_eq!(app.world().resource::<MapParentSeen>().asks.len(),if target_case=="same-window" {2}else{1},"fresh contact works after restoring the real primary target");
        let seen=app.world().resource::<DefaultMenuTouchSeen>();assert_eq!((seen.presses,seen.releases,seen.scrolls),(0,0,0));
    }
}
