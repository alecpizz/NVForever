use super::*;
use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},asset::{AssetApp,AssetPlugin,RenderAssetUsages},
    camera::{ComputedCameraValues,RenderTargetInfo,visibility::VisibilityPlugin},
    input::InputPlugin,render::render_resource::{Extent3d,TextureDimension,TextureFormat},
    ui::{ComputedUiRenderTargetInfo,ui_layout_system,ui_surface::UiSurface,update::propagate_ui_target_cameras}};
use crate::pipboy::{FlatScreen,PageBody,SCREEN_CROP};

#[derive(Resource,Default)]
struct Seen {asks:Vec<u32>,go:Vec<u32>,pip:Vec<PipBoyInput>}
fn hear(mut asks:MessageReader<FastTravelAsk>,mut go:MessageReader<FastTravelGo>,mut pip:MessageReader<PipBoyInput>,mut seen:ResMut<Seen>) {
    seen.asks.extend(asks.read().map(|m|m.0));seen.go.extend(go.read().map(|m|m.0));seen.pip.extend(pip.read().copied());
}
#[derive(Resource,Default)]struct TestGate {blocked:bool,layout:u64,consumed:usize}
fn ingress(mut bridge:Bridge,mut raw:MessageReader<TouchInput>,mut gate:ResMut<TestGate>) {
    bridge.prepare(!gate.blocked,gate.layout);
    for event in raw.read() {gate.consumed+=usize::from(bridge.event(event,true));}
}
fn map_state()->DataState {
    let map=WorldMap{root:1,transform:model::MapTransform::default(),texture:"authored-map.dds".into(),usable:[2048.0;2],nw:[-2,3],se:[1,2]};
    DataState {maps:[(1,Some(map))].into_iter().collect(),markers:Some(vec![read::Marker {
        form:0xF811,world:1,pos:[0.0,3.0*model::CELL,0.0],name:"Authored destination".into(),kind:1,flags:3,radius:10.0,reputation:None,
    }]),discovered:[0xF811].into_iter().collect(),..default()}
}
pub(crate) fn scene(tab:usize,density:f32,zoom:f32)->(App,Entity) {
    let mut app=App::new();app.add_plugins((MinimalPlugins,InputPlugin,AssetPlugin::default(),bevy::text::TextPlugin,bevy::transform::TransformPlugin))
        .init_asset::<Image>().init_asset::<Mesh>().init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
        .add_plugins(VisibilityPlugin)
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
        .init_resource::<UiSurface>().init_resource::<UiScale>().init_resource::<Kit>()
        .init_resource::<crate::pad::PadGlyphs>().init_resource::<PipBoyAid>()
        .init_resource::<TouchMap>().init_resource::<TestGate>().init_resource::<Seen>()
        .init_resource::<crate::touch_safe_area::SafeInsets>()
        .add_message::<TouchInput>().add_message::<MenuInput>().add_message::<PipBoyInput>()
        .add_message::<FastTravelAsk>().add_message::<FastTravelGo>()
        .add_message::<crate::pipboy::data::SummonCompanions>()
        .add_systems(PreUpdate,ingress.before(bevy::input::InputSystems))
        .add_systems(Update,(crate::pipboy::flat_screen,crate::pipboy::flat_pointer,draw,hear).chain())
        .add_systems(PostUpdate,(propagate_ui_target_cameras,ui_layout_system).chain())
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
    let size=UVec2::new((1024.0*density) as u32,(768.0*density) as u32);
    let mut window=Window{resolution:bevy::window::WindowResolution::new(size.x,size.y),..default()};window.resolution.set_scale_factor_override(Some(density));
    let window=app.world_mut().spawn((window,PrimaryWindow)).id();
    app.world_mut().spawn((Camera2d,bevy::ui::IsDefaultUiCamera,Camera{computed:ComputedCameraValues{
        target_info:Some(RenderTargetInfo{physical_size:size,scale_factor:density}),..default()},..default()}));
    let image=app.world_mut().resource_mut::<Assets<Image>>().add(Image::new_uninit(
        Extent3d{width:1024,height:768,depth_or_array_layers:1},TextureDimension::D2,TextureFormat::Rgba8UnormSrgb,RenderAssetUsages::default()));
    let camera=app.world_mut().spawn((Camera2d,Camera{computed:ComputedCameraValues{target_info:Some(RenderTargetInfo{physical_size:UVec2::new(1024,768),scale_factor:1.0}),..default()},..default()},
        bevy::camera::RenderTarget::Image(image.clone().into()))).id();
    let mut pip=PipBoy{phase:crate::pipboy::Phase::Up,page:Page::Data,frame:crate::pipboy::Frame::Screen,..default()};pip.tabs[Page::Data.index()]=tab;
    let sizing=crate::pipboy::touch_presentation::PipBoySize::for_platform(zoom!=1.0);
    assert!((sizing.ratio()-zoom).abs()<1e-6);
    app.insert_resource(sizing).insert_resource(pip).insert_resource(map_state())
        .insert_resource(PipBoyScreen{image,camera,on_model:false,pointer:None,scale:1.0})
        .insert_resource(crate::Location{world:1,interior:None})
        .insert_resource(crate::player::Player{heading:0.0,pitch:0.0,eye_height:0.0,feet:[0.0,3.0*model::CELL,0.0],body_eye:None,body_view:None,placed:None});
    app.world_mut().spawn((Node::default(),UiTargetCamera(camera),PageBody{page:Page::Data,tab}));
    settle(&mut app);(app,window)
}
pub(crate) fn settle(app:&mut App) {for _ in 0..5 {app.update();}}
pub(crate) fn raw_point(app:&mut App,canvas:Vec2)->Vec2 {
    let world=app.world_mut();let mut query=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<FlatScreen>>();
    let (node,at)=query.single(world).unwrap();let physical=at.translation-node.size()*0.5+node.size()*canvas/SCREEN_CROP;
    let mut query=world.query_filtered::<&Window,With<PrimaryWindow>>();physical/query.single(world).unwrap().scale_factor()
}
pub(crate) fn edge(app:&mut App,window:Entity,id:u64,phase:TouchPhase,raw:Vec2) {
    app.world_mut().write_message(TouchInput{window,id,phase,position:raw,force:None});
}
pub(crate) fn step_edge(app:&mut App,window:Entity,id:u64,phase:TouchPhase,raw:Vec2) {edge(app,window,id,phase,raw);app.update();}
pub(crate) fn shown(app:&App)->MapView {surface(app.world().resource::<DataState>(),app.world().resource::<PipBoy>().tab()).unwrap().view}
pub(crate) fn quiet(app:&App) {
    let seen=app.world().resource::<Seen>();assert!(seen.asks.is_empty()&&seen.go.is_empty()&&seen.pip.is_empty(),"map manipulation cannot become tabs/items/travel or gameplay commands");
}
#[test]
fn actual_flat_map_pan_uses_painted_canvas_at_both_densities_and_independent_content_sizes() {
    for tab in [LOCAL,WORLD] {for (density,zoom) in [(1.0,1.0),(2.0,1.1)] {
        let (mut app,window)=scene(tab,density,zoom);let before=shown(&app);let a=raw_point(&mut app,before.window.center());let b=a+Vec2::new(40.,20.);
        let from=app.world().resource::<TouchMap>().projected_point(a).unwrap()-before.window.min;
        let to=app.world().resource::<TouchMap>().projected_point(b).unwrap()-before.window.min;
        step_edge(&mut app,window,1,TouchPhase::Started,a);step_edge(&mut app,window,1,TouchPhase::Moved,b);
        let after=shown(&app);let want=before.touch_transform(1.0,from,to,1.0).unwrap().0;
        assert_eq!(app.world().resource::<DataState>().centre[tab],Some(want));assert_eq!(app.world().resource::<DataState>().magnify[tab],1.0);
        assert_ne!(after,before);step_edge(&mut app,window,1,TouchPhase::Ended,b);settle(&mut app);quiet(&app);
        assert!(!app.world().resource::<TouchMap>().captures());
    }}
}
#[test]
fn actual_flat_two_finger_pinch_keeps_midpoint_anchor_clamps_zoom_and_never_taps_on_release() {
    for tab in [LOCAL,WORLD] {
        let (mut app,window)=scene(tab,1.0,1.0);let before=shown(&app);let mid=raw_point(&mut app,before.window.center());
        let a=mid-Vec2::X*40.;let b=mid+Vec2::X*40.;
        edge(&mut app,window,10,TouchPhase::Started,a);edge(&mut app,window,11,TouchPhase::Started,b);app.update();
        step_edge(&mut app,window,10,TouchPhase::Moved,mid-Vec2::X*160.);
        step_edge(&mut app,window,11,TouchPhase::Moved,mid+Vec2::X*160.);
        let after=shown(&app);assert!((app.world().resource::<DataState>().magnify[tab]-4.0).abs()<1e-5);
        let old=before.local_point(before.mid());let new=after.local_point(after.mid());
        if tab==LOCAL {let (old,new)=(old.unwrap(),new.unwrap());assert!((Vec2::from(old)-Vec2::from(new)).length()<0.1);}
        else {assert!((Vec2::from(before.to_uv(before.mid()))-Vec2::from(after.to_uv(after.mid()))).length()<1e-5);}
        edge(&mut app,window,10,TouchPhase::Ended,mid-Vec2::X*160.);edge(&mut app,window,11,TouchPhase::Ended,mid+Vec2::X*160.);app.update();settle(&mut app);quiet(&app);
    }
}
#[test]
fn actual_world_marker_tap_requests_only_existing_question_and_repainted_identity_cannot_retarget() {
    let (mut app,window)=scene(WORLD,1.0,1.0);let centre=shown(&app).window.center();let p=raw_point(&mut app,centre);
    edge(&mut app,window,20,TouchPhase::Started,p);edge(&mut app,window,20,TouchPhase::Ended,p);app.update();
    assert_eq!(app.world().resource::<Seen>().asks,[0xF811]);assert!(app.world().resource::<Seen>().go.is_empty());
    app.world_mut().resource_mut::<Seen>().asks.clear();
    step_edge(&mut app,window,21,TouchPhase::Started,p);
    app.world_mut().resource_mut::<DataState>().markers.as_mut().unwrap()[0].form=0xF812;app.update();
    step_edge(&mut app,window,21,TouchPhase::Ended,p);settle(&mut app);quiet(&app);
    edge(&mut app,window,22,TouchPhase::Started,p);edge(&mut app,window,22,TouchPhase::Ended,p);app.update();
    assert_eq!(app.world().resource::<Seen>().asks,[0xF812]);assert!(app.world().resource::<Seen>().go.is_empty());
}
#[test]
fn actual_map_cancel_context_and_third_finger_retire_edits_and_require_fresh_contact() {
    for cause in ["cancel","focus","controller","platform","safe","layout","third"] {
        let (mut app,window)=scene(WORLD,1.0,1.0);let before=shown(&app);let p=raw_point(&mut app,before.window.center());
        step_edge(&mut app,window,30,TouchPhase::Started,p);
        match cause {
            "cancel"=>edge(&mut app,window,30,TouchPhase::Canceled,p),
            "focus"=>app.world_mut().get_mut::<Window>(window).unwrap().focused=false,
            "controller"=>{let mut bindings=Bindings::default();bindings.device=InputDevice::Controller;app.insert_resource(bindings);},
            "platform"=>{let mut gate=crate::pad::PlatformInputGate::default();gate.set_blocked(true);app.insert_resource(gate);},
            "safe"=>app.world_mut().resource_mut::<crate::touch_safe_area::SafeInsets>().left=12.0,
            "layout"=>app.world_mut().resource_mut::<TestGate>().layout+=1,
            _=>{edge(&mut app,window,31,TouchPhase::Started,p+Vec2::X*40.);edge(&mut app,window,32,TouchPhase::Started,p-Vec2::X*40.);},
        }
        edge(&mut app,window,30,TouchPhase::Moved,p+Vec2::X*60.);edge(&mut app,window,30,TouchPhase::Ended,p+Vec2::X*60.);
        edge(&mut app,window,31,TouchPhase::Ended,p);edge(&mut app,window,32,TouchPhase::Ended,p);app.update();
        assert_eq!(app.world().resource::<DataState>().centre[WORLD],Some(before.centre));quiet(&app);
        app.world_mut().get_mut::<Window>(window).unwrap().focused=true;
        app.world_mut().remove_resource::<Bindings>();app.world_mut().remove_resource::<crate::pad::PlatformInputGate>();
        app.world_mut().resource_mut::<crate::touch_safe_area::SafeInsets>().left=0.0;settle(&mut app);
        let centre=shown(&app).window.center();let p=raw_point(&mut app,centre);
        edge(&mut app,window,33,TouchPhase::Started,p);edge(&mut app,window,33,TouchPhase::Ended,p);app.update();
        assert_eq!(app.world().resource::<Seen>().asks,[0xF811],"{cause}: a fresh real contact must remain usable");
    }
}

#[test]
fn actual_marker_request_obeys_native_travel_refusal_and_confirmation_without_direct_departure() {
    for blocked in [false,true] {
        let (mut app,window)=scene(WORLD,1.0,1.0);
        let mut script=crate::script::ScriptState::over_no_plugins();
        script.rt.as_mut().unwrap().game.player.fast_travel.travel_disabled=blocked;
        app.insert_resource(script).insert_resource(crate::sim::test_world::sim(vec![],vec![]))
            .insert_resource(crate::script_draw::tests::play_over(&[]))
            .init_resource::<crate::phys::PhysState>().init_resource::<crate::hitvol::HitVolumes>()
            .add_systems(Update,travel::answer.after(draw).before(hear));
        let centre=shown(&app).window.center();let point=raw_point(&mut app,centre);
        edge(&mut app,window,50,TouchPhase::Started,point);edge(&mut app,window,50,TouchPhase::Ended,point);app.update();
        assert_eq!(app.world().resource::<Seen>().asks,[0xF811]);
        assert!(app.world().resource::<Seen>().go.is_empty(),"a marker tap only asks; it never bypasses confirmation");
        let popup=app.world().resource::<DataState>().popup.as_ref().unwrap();
        if blocked {assert!(matches!(popup.ask,Ask::Refused));}else{assert!(matches!(popup.ask,Ask::Travel(0xF811)));}
        // The actual existing popup consumer takes the native menu confirmation.
        app.world_mut().write_message(MenuInput(Command::One));app.update();
        assert_eq!(app.world().resource::<Seen>().go,if blocked {vec![]}else{vec![0xF811]});
        assert!(app.world().resource::<Seen>().pip.is_empty(),"touch cannot put the device away or teleport by itself");
    }
}
#[test]
fn actual_map_raw_batch_after_release_is_drained_and_real_mouse_priority_cannot_resume_old_touch() {
    let (mut app,window)=scene(WORLD,1.0,1.0);let before=shown(&app);let point=raw_point(&mut app,before.window.center());
    step_edge(&mut app,window,60,TouchPhase::Started,point);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    edge(&mut app,window,60,TouchPhase::Moved,point+Vec2::X*60.);app.update();
    assert_eq!(app.world().resource::<DataState>().centre[WORLD],Some(before.centre));
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    edge(&mut app,window,60,TouchPhase::Ended,point);app.update();settle(&mut app);quiet(&app);
    let centre=shown(&app).window.center();let point=raw_point(&mut app,centre);
    let prior=app.world().resource::<TestGate>().consumed;
    edge(&mut app,window,61,TouchPhase::Started,point);edge(&mut app,window,61,TouchPhase::Ended,point);
    edge(&mut app,window,62,TouchPhase::Started,Vec2::new(1.,1.));edge(&mut app,window,62,TouchPhase::Ended,Vec2::new(1.,1.));app.update();
    assert_eq!(app.world().resource::<TestGate>().consumed-prior,4,"a map release quarantines the rest of its raw batch");
    assert_eq!(app.world().resource::<Seen>().asks,[0xF811]);
    assert!(app.world().resource::<Seen>().go.is_empty()&&app.world().resource::<Seen>().pip.is_empty());
}

#[test]
fn real_raw_contact_flood_stays_bounded_quarantines_until_inputplugin_neutral_then_accepts_fresh() {
    let (mut app,window)=scene(WORLD,1.0,1.0);let before=shown(&app);let point=raw_point(&mut app,before.window.center());
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
    for frame in 0..3 {for n in 0..(MAX_IDS+5) {
        edge(&mut app,window,1000+(frame*(MAX_IDS+5)+n) as u64,TouchPhase::Started,point+Vec2::X*n as f32);
    }app.update();
        let state=app.world().resource::<TouchMap>();assert!(state.overflow&&state.captures());
        assert!(state.ids.len()<=MAX_IDS&&state.blocked.len()<=MAX_IDS&&state.edits.len()<=MAX_EDITS);
        assert!(state.fingers.iter().all(Option::is_none));assert!(state.edits.is_empty());
        assert_eq!(app.world().resource::<DataState>().centre[WORLD],Some(before.centre));quiet(&app);
    }
    assert_eq!(app.world().resource::<bevy::input::touch::Touches>().iter().count(),3*(MAX_IDS+5),"only actual InputPlugin owns the neutral witness");
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    for n in 0..3*(MAX_IDS+5) {edge(&mut app,window,1000+n as u64,TouchPhase::Ended,point);}
    app.update();assert!(app.world().resource::<bevy::input::touch::Touches>().iter().next().is_none());
    settle(&mut app);quiet(&app);assert!(!app.world().resource::<TouchMap>().captures());
    edge(&mut app,window,2000,TouchPhase::Started,point);edge(&mut app,window,2000,TouchPhase::Ended,point);app.update();
    assert_eq!(app.world().resource::<Seen>().asks,[0xF811]);
    assert!(app.world().resource::<Seen>().go.is_empty());assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
}

#[derive(Resource,Default)]struct LateContactGate(Option<u64>);
fn quarantine_after_raw_end(world:&mut World) {
    let Some(id)=world.resource_mut::<LateContactGate>().0.take() else {return;};
    assert!(world.resource::<bevy::input::touch::Touches>().get_pressed(id).is_some(),"actual InputPlugin still owns the prior Started before this frame's Ended update");
    world.resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
    crate::pad::device::tests::sample_platform_holds_now(world);
    let mut gate=world.resource_mut::<crate::pad::PlatformInputGate>();gate.set_blocked(false);
    assert!(gate.allows_actions());assert!(!gate.allows_touch(id),"the actual hold sampler quarantined this exact contact");
}
#[test]
fn actual_ended_batch_revalidates_its_id_after_late_platform_quarantine_for_tap_and_pan() {
    for pan in [false,true] {
        let (mut app,window)=scene(WORLD,1.0,1.0);let before=shown(&app);let point=raw_point(&mut app,before.window.center());
        app.init_resource::<crate::pad::PlatformInputGate>().init_resource::<LateContactGate>()
            .add_systems(PreUpdate,quarantine_after_raw_end.after(ingress).before(bevy::input::InputSystems));
        step_edge(&mut app,window,88,TouchPhase::Started,point);
        app.world_mut().resource_mut::<LateContactGate>().0=Some(88);
        if pan {edge(&mut app,window,88,TouchPhase::Moved,point+Vec2::X*60.);}
        edge(&mut app,window,88,TouchPhase::Ended,if pan {point+Vec2::X*60.}else{point});app.update();
        assert!(app.world().resource::<TouchMap>().fingers.iter().all(Option::is_none));
        assert!(app.world().resource::<crate::pad::PlatformInputGate>().allows_actions());
        assert!(!app.world().resource::<crate::pad::PlatformInputGate>().allows_touch(88));
        assert_eq!(app.world().resource::<DataState>().centre[WORLD],Some(before.centre),"late per-ID denial must reject the complete terminal pan batch");quiet(&app);
        settle(&mut app);edge(&mut app,window,89,TouchPhase::Started,point);edge(&mut app,window,89,TouchPhase::Ended,point);app.update();
        assert_eq!(app.world().resource::<Seen>().asks,[0xF811],"a fresh nonquarantined contact must work once");
        assert!(app.world().resource::<Seen>().go.is_empty());
    }
}

pub(crate) fn handbuilt_window()->Rect {super::super::hand_built_window()}
pub(crate) fn magnification(app:&App,tab:usize)->f32 {app.world().resource::<DataState>().magnify[tab]}

#[test]
fn actual_shared_view_transform_holds_both_map_bounds_rejects_invalid_inputs_and_anchors_off_centre() {
    let window=Rect::new(50.,100.,905.,600.);
    for mag in [1.0,2.0,4.0] {
        let world=MapView::world(window,[0.4,0.6],Vec2::splat(2048.),mag);
        let local=MapView::local(window,view::LocalProjection{centre:[0.,0.],mag,scale:28.,origin:[-40000.,40000.],extent:Vec2::splat(80000.)});
        for before in [world,local] {
            let from=before.mid()+Vec2::new(80.,-30.);let to=from+Vec2::new(20.,12.);
            let (centre,next)=before.touch_transform(mag,from,to,1.5).unwrap();
            let after=if let Some(p)=before.local {MapView::local(window,view::LocalProjection{centre,mag:next,..p})}else{MapView::world(window,centre,Vec2::splat(2048.),next)};
            if before.local.is_some() {assert!((Vec2::from(before.local_point(from).unwrap())-Vec2::from(after.local_point(to).unwrap())).length()<0.1);}
            else {assert!((Vec2::from(before.to_uv(from))-Vec2::from(after.to_uv(to))).length()<1e-5);}
            for ratio in [0.00001,100000.] {
                let (centre,zoom)=before.touch_transform(mag,from,Vec2::splat(1e9),ratio).unwrap();
                assert!((1.0..=4.0).contains(&zoom));
                if let Some(p)=before.local {assert!((p.origin[0]..=p.origin[0]+p.extent.x).contains(&centre[0]));assert!((p.origin[1]-p.extent.y..=p.origin[1]).contains(&centre[1]));}
                else {assert!(centre.into_iter().all(|v|(0.0..=1.0).contains(&v)));}
            }
            for invalid in [0.0,-1.0,f32::NAN,f32::INFINITY] {assert!(before.touch_transform(mag,from,to,invalid).is_none());}
        }
    }
}

#[test]
fn actual_terminal_edit_overflow_cannot_recreate_even_an_empty_map_selection_tap() {
    let (mut app,window)=scene(WORLD,1.0,1.0);let centre=shown(&app).window.center();let marker=raw_point(&mut app,centre);
    edge(&mut app,window,4000,TouchPhase::Started,marker);edge(&mut app,window,4000,TouchPhase::Ended,marker);app.update();settle(&mut app);
    assert_eq!(app.world().resource::<DataState>().travel_marker,Some(0xF811));
    app.world_mut().resource_mut::<Seen>().asks.clear();
    let before=shown(&app);let empty=raw_point(&mut app,centre+Vec2::new(150.,100.));
    edge(&mut app,window,4001,TouchPhase::Started,empty);
    for n in 0..MAX_EDITS {edge(&mut app,window,4001,TouchPhase::Moved,empty+Vec2::X*(20.+n as f32));}
    // The terminal transform itself exceeds the bound. Its failure must return
    // before cancellation's reset moved/marker fields can create Tap(None).
    edge(&mut app,window,4001,TouchPhase::Ended,empty+Vec2::X*50.);app.update();
    let state=app.world().resource::<TouchMap>();assert!(state.overflow);assert!(state.edits.is_empty());
    assert_eq!(shown(&app),before);assert_eq!(app.world().resource::<DataState>().travel_marker,Some(0xF811));quiet(&app);
    settle(&mut app);edge(&mut app,window,4002,TouchPhase::Started,marker);edge(&mut app,window,4002,TouchPhase::Ended,marker);app.update();
    assert_eq!(app.world().resource::<Seen>().asks,[0xF811],"neutral witness permits a genuinely fresh contact after overflow");
}

#[derive(Resource,Default)]struct LateMapModal(Option<&'static str>);
fn open_late_map_modal(world:&mut World) {
    use fnv_game::game::menus;
    let Some(which)=world.resource_mut::<LateMapModal>().0.take() else {return;};
    {
        let mut script=world.resource_mut::<ScriptState>();let game=&mut script.rt.as_mut().unwrap().game;
        match which {
            "message"=>game.open_menu(menus::MESSAGE),
            "under"=>{game.menu=None;game.menu_under=Some(menus::MESSAGE);},
            "loading"=>game.loading=true,
            "vats"=>game.vats.mode=fnv_game::vats::MODE_TARGET_SELECT,
            "creator"=>game.open_menu(menus::RACE),
            _=>unreachable!(),
        }
    }
    if which=="creator" {
        crate::creator::synchronize_for_map_control(world);
        assert!(world.resource::<crate::creator::CreatorMenu>().is_open(),"real creator sync constructed the real model");
        // The core renderer has not run sync again yet; its visible creator still
        // owns this frame even if the script restores the underlying MAP code.
        world.resource_mut::<ScriptState>().rt.as_mut().unwrap().game.menu=Some(menus::MAP);
    }
}
#[test]
fn actual_late_script_side_menu_and_creator_ownership_cancels_map_tap_then_own_map_accepts_fresh_contact() {
    use fnv_game::game::menus;
    for which in ["message","under","loading","vats","creator"] {
        let (mut app,window)=scene(WORLD,1.0,1.0);
        let mut script=ScriptState::over_no_plugins();script.rt.as_mut().unwrap().game.menu=Some(menus::MAP);
        let data=fnv_actor::creator::Creator{player:Some(fnv_actor::Look {
            race:fnv_sim::FormId(0x19),female:false,hair:None,hair_color:None,eyes:None,head_parts:vec![],facegen:default(),
        }),..default()};
        app.insert_resource(script).insert_resource(crate::creator::CreatorData(Arc::new(data)))
            .init_resource::<crate::creator::CreatorMenu>().init_resource::<crate::control::PlayerControl>()
            .init_resource::<LateMapModal>().add_systems(Update,open_late_map_modal.before(draw));
        settle(&mut app);let before=shown(&app);let point=raw_point(&mut app,before.window.center());
        step_edge(&mut app,window,4100,TouchPhase::Started,point);
        assert!(app.world().resource::<TouchMap>().captures());
        app.world_mut().resource_mut::<LateMapModal>().0=Some(which);
        edge(&mut app,window,4100,TouchPhase::Ended,point);app.update();
        quiet(&app);assert_eq!(shown(&app),before,"{which}: the late modal cannot admit a transform or marker request");
        {
            let mut script=app.world_mut().resource_mut::<ScriptState>();let game=&mut script.rt.as_mut().unwrap().game;
            game.menu=Some(menus::MAP);game.menu_under=None;game.loading=false;game.vats.mode=0;
        }
        crate::creator::synchronize_for_map_control(app.world_mut());
        assert!(!app.world().resource::<crate::creator::CreatorMenu>().is_open());
        settle(&mut app);edge(&mut app,window,4101,TouchPhase::Started,point);edge(&mut app,window,4101,TouchPhase::Ended,point);app.update();
        assert_eq!(app.world().resource::<Seen>().asks,[0xF811],"{which}: the ordinary PipBoy MAP route remains usable after closure");
        assert!(app.world().resource::<Seen>().go.is_empty());
    }
}
