//! Actual hand-built frame, Taffy, flat projection, picking and typed page input.
use super::*;
use crate::pipboy::{self,touch_items::TouchItems,touch_scroll::TouchPointerPlugin};
use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},camera::{ComputedCameraValues,RenderTarget,RenderTargetInfo,visibility::VisibilityPlugin},
    picking::{PickingPlugin,InteractionPlugin,PickingSystems,backend::{PointerHits,HitData},pointer::{PointerId,PointerLocation}},
    text::{TextPipeline,FontCx,RemSize,ScaleCx},ui::{ComputedUiTargetCamera,ComputedUiRenderTargetInfo,UiScale,ui_surface::UiSurface,update::propagate_ui_target_cameras,sync_font_size_to_em_size,ui_layout_system},window::PrimaryWindow};
#[derive(Resource,Default)]
struct Seen(Vec<PipBoyInput>);
fn see(mut input:MessageReader<PipBoyInput>,mut seen:ResMut<Seen>) {seen.0.extend(input.read().copied());}
fn backend(pointers:Query<(&PointerId,&PointerLocation)>,nodes:Query<(Entity,&ComputedNode,&UiGlobalTransform),With<pipboy::Clickable>>,screen:Res<pipboy::PipBoyScreen>,mut hits:MessageWriter<PointerHits>) {
    for (id,location) in &pointers {
        if *id!=pipboy::touch_items::POINTER {continue}
        let Some(location)=location.location() else {continue};
        let position=location.position*screen.scale;
        let targets=nodes.iter().filter(|(_,n,t)|Rect::from_center_size(t.transform_point2(Vec2::ZERO),n.size()).contains(position))
            .map(|(e,_,_)|(e,HitData::new(screen.camera,0.0,None,None))).collect();
        hits.write(PointerHits::new(*id,targets,0.0));
    }
}
fn scene(scale:f32)->(App,Entity) {
    let mut app=App::new();
    app.add_plugins((MinimalPlugins,bevy::input::InputPlugin,PickingPlugin,InteractionPlugin,bevy::transform::TransformPlugin))
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_plugins(VisibilityPlugin)
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
        .init_resource::<UiSurface>().init_resource::<UiScale>().init_resource::<TextPipeline>().init_resource::<FontCx>().init_resource::<RemSize>().init_resource::<ScaleCx>()
        .init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>().init_resource::<pipboy::frame::Kit>()
        .init_resource::<crate::bindings::Bindings>().init_resource::<TouchItems>().init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>()
        .init_resource::<Seen>().add_message::<pipboy::sounds::TabHovered>()
        .add_plugins(TouchPointerPlugin);
    app.insert_resource(Options {enabled:true});
    let pip=PipBoy {phase:Phase::Up,page:Page::Items,tabs:[0,2,0],frame:pipboy::Frame::Screen,..default()};
    app.insert_resource(pip);
    let window=app.world_mut().spawn((Window {resolution:bevy::window::WindowResolution::new(1000,800),..default()},PrimaryWindow)).id();
    app.world_mut().spawn((Camera2d,Camera {computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {physical_size:UVec2::new(1000,800),scale_factor:1.0}),..default()},..default()}));
    let camera=app.world_mut().spawn((Camera2d,Camera {computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {physical_size:(pipboy::TARGET.as_vec2()*scale).as_uvec2(),scale_factor:scale}),..default()},..default()},RenderTarget::Image(bevy::camera::ImageRenderTarget{handle:Handle::default(),scale_factor:scale}))).id();
    app.insert_resource(pipboy::PipBoyScreen {image:Handle::default(),camera,on_model:false,pointer:None,scale});
    app.world_mut().spawn(pipboy::touch_items::POINTER);
    app.add_systems(PreUpdate,backend.in_set(PickingSystems::Backend))
        .add_systems(Update,(pipboy::apply_input,pipboy::frame::draw,pipboy::flat_screen,see).chain())
        .add_systems(Last,pipboy::flat_pointer)
        .add_systems(PostUpdate,(ApplyDeferred,propagate_ui_target_cameras,sync_font_size_to_em_size,ui_layout_system).chain());
    app.configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
    app.configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
    flush(&mut app);
    (app,window)
}
fn flush(app:&mut App) {for _ in 0..6 {app.update();}}
fn point(app:&mut App,kind:Kind)->Vec2 {
    let scale=app.world().resource::<pipboy::PipBoyScreen>().scale;
    let mut query=app.world_mut().query::<(&Strip,&ComputedNode,&UiGlobalTransform,&pipboy::Clickable)>();
    let canvas=query.iter(app.world()).find(|(s,_,_,click)|s.0==kind && matches!((kind,click.0),(Kind::Sections,PipBoyInput::Page(Page::Items))|(Kind::Subtabs,PipBoyInput::Tab(1))))
        .map(|(_,n,t,_)|Rect::from_center_size(t.transform_point2(Vec2::ZERO),n.size()).center()/scale).expect("actual production navigation control");
    let mut flat=app.world_mut().query_filtered::<(&ComputedNode,&UiGlobalTransform),With<pipboy::FlatScreen>>();
    let (node,at)=flat.single(app.world()).unwrap();
    at.transform_point2(Vec2::ZERO)-node.size()*0.5+canvas/pipboy::SCREEN_CROP*node.size()
}
fn event(app:&mut App,window:Entity,phase:TouchPhase,p:Vec2,ms:u64) {
    assert!(app.world_mut().resource_mut::<TouchItems>().event(&bevy::input::touch::TouchInput {window,id:320,phase,position:p,force:None},ms));
}
fn swipe(app:&mut App,window:Entity,p:Vec2,delta:Vec2) {
    event(app,window,TouchPhase::Started,p,0);app.update();app.update();
    event(app,window,TouchPhase::Moved,p+delta,30);event(app,window,TouchPhase::Ended,p+delta,80);flush(app);
}
#[test]
fn real_handbuilt_title_and_tab_swipes_use_logical_threshold_across_image_scales() {
    for scale in [1.0,1.5,2.0] {
        for kind in [Kind::Sections,Kind::Subtabs] {
            for distance in [30.0,60.0] {
                let (mut app,window)=scene(scale);
                let p=point(&mut app,kind);
                swipe(&mut app,window,p,-Vec2::X*distance);
                let pip=app.world().resource::<PipBoy>();
                assert_eq!((pip.page,pip.tab()),if distance<48.0 {(Page::Items,2)}else if kind==Kind::Sections {(Page::Data,0)}else{(Page::Items,3)},"{scale}/{kind:?}/{distance}");
                assert_eq!(app.world().resource::<Seen>().0.len(),usize::from(distance>=48.0));
            }
        }
    }
}
#[test]
fn late_capture_drains_navigation_request_and_fresh_tap_keeps_existing_click_route() {
    #[derive(Resource,Default)]struct Block(bool);
    fn capture(mut block:ResMut<Block>,mut overlay:ResMut<pipboy::touch_items::OverlayCapture>) {if block.0 {overlay.0=true;block.0=false;}}
    let (mut app,window)=scene(1.0);
    app.init_resource::<Block>().init_resource::<pipboy::touch_items::OverlayCapture>()
        .add_systems(Update,capture.before(Systems::Dispatch));
    let p=point(&mut app,Kind::Subtabs);
    event(&mut app,window,TouchPhase::Started,p,0);app.update();app.update();
    event(&mut app,window,TouchPhase::Moved,p-Vec2::X*60.0,30);app.update();app.update();
    event(&mut app,window,TouchPhase::Ended,p-Vec2::X*60.0,80);
    // First update projects Ended; next PreUpdate queues it, then capture wins in Update.
    app.update();app.world_mut().resource_mut::<Block>().0=true;app.update();
    assert_eq!(app.world().resource::<PipBoy>().tab(),2);
    app.world_mut().resource_mut::<pipboy::touch_items::OverlayCapture>().0=false;
    flush(&mut app);
    assert!(app.world().resource::<Seen>().0.is_empty(),"request must not replay when overlay closes");
    let p=point(&mut app,Kind::Subtabs);
    event(&mut app,window,TouchPhase::Started,p,100);app.update();app.update();
    event(&mut app,window,TouchPhase::Ended,p,150);flush(&mut app);
    // The point is the actual second-tab control, independent of query ordering.
    assert_eq!(app.world().resource::<PipBoy>().tab(),1);
    assert_eq!(app.world().resource::<Seen>().0.len(),1);
}
#[test]
fn vertical_strip_drag_and_geometry_change_never_turn_into_navigation_or_click() {
    for changed in [false,true] {
        let (mut app,window)=scene(1.0);let p=point(&mut app,Kind::Subtabs);
        event(&mut app,window,TouchPhase::Started,p,0);app.update();app.update();
        if changed {app.world_mut().resource_mut::<pipboy::PipBoyScreen>().scale=1.5;}
        event(&mut app,window,TouchPhase::Moved,p-Vec2::Y*60.0,30);
        event(&mut app,window,TouchPhase::Ended,p,80);flush(&mut app);
        assert_eq!(app.world().resource::<PipBoy>().tab(),2);assert!(app.world().resource::<Seen>().0.is_empty());
    }
}
#[test]
fn swipe_preference_roundtrips_via_existing_ini_without_changing_other_controls() {
    let path=std::env::temp_dir().join(format!("nvf-tab-swipes-{}-{}.ini",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    for enabled in [false,true] {
        let mut app=App::new();install(&mut app);app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        app.insert_resource(Options {enabled:!enabled});
        let index=app.world().resource::<crate::settings::Settings>().index(KEY).unwrap();
        assert_eq!(crate::settings::flip(app.world_mut(),index),Some(enabled));
        assert!(std::fs::read_to_string(&path).unwrap().contains(&format!("{KEY}={}",usize::from(enabled))));
        let mut loaded=App::new();install(&mut loaded);loaded.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(loaded.world_mut());assert_eq!(loaded.world().resource::<Options>().enabled,enabled);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn actual_mouse_hold_cancels_touch_navigation_without_clearing_hardware_and_fresh_touch_works() {
    use bevy::input::{ButtonState,mouse::MouseButtonInput};
    let (mut app,window)=scene(1.0);let p=point(&mut app,Kind::Subtabs);
    event(&mut app,window,TouchPhase::Started,p,0);app.update();app.update();
    event(&mut app,window,TouchPhase::Moved,p-Vec2::X*60.0,30);
    event(&mut app,window,TouchPhase::Ended,p-Vec2::X*60.0,80);
    app.world_mut().write_message(MouseButtonInput {button:MouseButton::Left,state:ButtonState::Pressed,window});
    flush(&mut app);
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert!(app.world().resource::<Seen>().0.is_empty());assert_eq!(app.world().resource::<PipBoy>().tab(),2);
    app.world_mut().write_message(MouseButtonInput {button:MouseButton::Left,state:ButtonState::Released,window});flush(&mut app);
    let p=point(&mut app,Kind::Subtabs);swipe(&mut app,window,p,-Vec2::X*60.0);
    assert_eq!(app.world().resource::<PipBoy>().tab(),3);assert_eq!(app.world().resource::<Seen>().0.len(),1);
}

#[test]
fn actual_script_menu_opened_after_projection_retires_request_before_native_apply_and_no_replay() {
    #[derive(Resource,Default)]struct Open(bool);
    fn open(mut ask:ResMut<Open>,mut script:ResMut<crate::script::ScriptState>) {
        if ask.0 {script.rt.as_mut().unwrap().game.menu=Some(fnv_game::game::menus::MESSAGE);ask.0=false;}
    }
    let (mut app,window)=scene(1.0);
    app.insert_resource(crate::script::ScriptState::over_no_plugins()).init_resource::<Open>()
        .add_systems(Update,open.before(Systems::Dispatch));
    let p=point(&mut app,Kind::Subtabs);
    event(&mut app,window,TouchPhase::Started,p,0);app.update();app.update();
    event(&mut app,window,TouchPhase::Moved,p-Vec2::X*60.0,30);app.update();app.update();
    event(&mut app,window,TouchPhase::Ended,p-Vec2::X*60.0,80);app.update();
    app.world_mut().resource_mut::<Open>().0=true;app.update();
    assert!(app.world().resource::<Seen>().0.is_empty());assert_eq!(app.world().resource::<PipBoy>().tab(),2);
    app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.menu=None;
    flush(&mut app);
    assert!(app.world().resource::<Seen>().0.is_empty(),"closing a real script menu must not replay navigation");
    let p=point(&mut app,Kind::Subtabs);swipe(&mut app,window,p,-Vec2::X*60.0);
    assert_eq!(app.world().resource::<PipBoy>().tab(),3);assert_eq!(app.world().resource::<Seen>().0.len(),1);
}

#[test]
fn actual_handbuilt_header_uses_transformed_clip_stack_and_rejects_fully_clipped() {
    use bevy::ui::CalculatedClip;
    for mode in 0..3 {
        let (mut app,window)=scene(1.0);
        let point=point(&mut app,Kind::Subtabs);
        let (entity,center)={
            let world=app.world_mut();
            let mut q=world.query::<(Entity,&Strip,&ComputedNode,&UiGlobalTransform,&pipboy::Clickable)>();
            q.iter(world).find(|(_,s,_,_,c)|s.0==Kind::Subtabs && c.0==PipBoyInput::Tab(1))
                .map(|(entity,_,_,at,_)|(entity,at.transform_point2(Vec2::ZERO))).unwrap()
        };
        let offset=Vec2::new(-300.0,200.0);
        let mut clip=CalculatedClip::default();
        if mode==0 {clip=CalculatedClip::FullyClipped;} else {
            let world_center=if mode==1 {center+Vec2::splat(1000.0)}else{center};
            clip.push_rect(Rect::from_center_size(world_center+offset,Vec2::new(500.0,100.0)),bevy::math::Affine2::from_translation(offset));
            assert_eq!(clip.contains_point(center),mode==2,"fixture must test transformed actual admission");
            assert!(clip.rects().is_some_and(|rects|rects.len()==1));
        }
        app.world_mut().entity_mut(entity).insert(clip);
        swipe(&mut app,window,point,-Vec2::X*60.0);
        assert_eq!(app.world().resource::<PipBoy>().tab(),if mode==2 {3}else{2});
        assert_eq!(app.world().resource::<Seen>().0.len(),usize::from(mode==2));
    }
}

include!("manual_scroll_tests.rs");
