// Existing production reader + actual flat screen/Taffy + actual MenuPlugin.
// No direct Bridge injection, test-only target publication or close effect writer.
fn diagonal_switch(app:&mut App,key:&str,value:bool) {
    let row=app.world().resource::<nvf_play::settings::Settings>().index(key).unwrap();
    let before=(app.world().resource::<nvf_play::settings::Settings>().list()[row].get)(app.world());
    if before!=value {assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(value));}
}
fn diagonal_resize(app:&mut App,window:Entity,size:Vec2,density:f32,scale:f32) {
    use bevy::camera::RenderTargetInfo;
    let physical=(size*density).as_uvec2();
    let mut resolution=bevy::window::WindowResolution::new(physical.x,physical.y);
    resolution.set_scale_factor_override(Some(density));
    app.world_mut().get_mut::<Window>(window).unwrap().resolution=resolution;
    app.insert_resource(UiScale(scale));
    let camera=app.world().resource::<nvf_play::pipboy::PipBoyScreen>().camera;
    app.world_mut().get_mut::<Camera>(camera).unwrap().computed.target_info=Some(RenderTargetInfo {physical_size:physical,scale_factor:density});
    for _ in 0..6 {app.update();}
}
fn diagonal_flat(app:&mut App)->Rect {
    let density={let world=app.world_mut();let mut q=world.query_filtered::<&Window,With<PrimaryWindow>>();q.single(world).unwrap().scale_factor()};
    let world=app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<nvf_play::pipboy::FlatScreen>>();
    let (node,at)=q.single(world).unwrap();assert!(node.size().min_element()>0.);
    Rect::from_center_size(at.translation/density,node.size()/density)
}
fn diagonal_painted(app:&mut App,left:bool)->Rect {
    let density={let world=app.world_mut();let mut q=world.query_filtered::<&Window,With<PrimaryWindow>>();q.single(world).unwrap().scale_factor()};
    let world=app.world_mut();let mut q=world.query::<(&Text,&ChildOf)>();
    let mut rects=q.iter(world).filter(|(text,_)|text.0=="Drag down").map(|(_,parent)|{
        let node=world.get::<ComputedNode>(parent.parent()).unwrap();let at=world.get::<UiGlobalTransform>(parent.parent()).unwrap();
        Rect::from_center_size(at.translation/density,node.size()/density)
    }).collect::<Vec<_>>();
    assert_eq!(rects.len(),2,"both enabled handles must have real painted usable regions");
    rects.sort_by(|a,b|a.center().x.total_cmp(&b.center().x));
    let r=rects[usize::from(!left)];assert!(r.size().abs_diff_eq(Vec2::new(48.,96.),0.5));r
}
fn diagonal_flat_scene(size:Vec2,density:f32,scale:f32,graphics:bool)->(App,Entity) {
    let (mut app,w)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
    app.init_resource::<nvf_play::pad::PlatformInputGate>();
    pipboy_drag::tests::install_native_close_consumer(&mut app);
    app.insert_resource(nvf_play::touch_safe_area::SafeInsets {left:18.,top:16.,right:22.,bottom:26.});
    diagonal_switch(&mut app,"touch-pipboy-dismiss-left",true);
    diagonal_switch(&mut app,"touch-pipboy-dismiss-right",true);
    diagonal_switch(&mut app,"touch-pipboy-button-graphics",graphics);
    app.insert_resource(nvf_play::pipboy::touch_presentation::PipBoySize::for_platform(true));
    app.world_mut().resource_mut::<Layout>().config.southpaw=scale>1.0;
    diagonal_resize(&mut app,w,size,density,scale);
    (app,w)
}
#[test]
fn actual_flat_both_down_handles_keep_visible_geometry_and_diagonal_native_close_at_phone_scales() {
    for (size,density,scale) in [
        (Vec2::new(400.,300.),1.,0.75),(Vec2::new(640.,360.),1.,1.),
        (Vec2::new(792.,363.),2.,1.5),(Vec2::new(400.,300.),2.,1.5),
        (Vec2::splat(1000.),1.,1.),
    ] {for left in [true,false] {for graphics in [true,false] {
        let (mut app,w)=diagonal_flat_scene(size,density,scale,graphics);
        let flat=diagonal_flat(&mut app);let region=diagonal_painted(&mut app,left);
        let overlap=flat.intersect(region);assert!(overlap.width()<=0. || overlap.height()<=0.,"dismissal cannot cover selectable canvas");
        let p=region.center();let travel=size.y-26.-p.y;
        let end=p+Vec2::new(if left {60.} else {-60.},travel*0.8);
        assert!(end.y<=size.y-26. && end.x>=18. && end.x<=size.x-22.);
        touch(&mut app,w,1500,TouchPhase::Started,p.x,p.y);app.update();
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(1500));
        touch(&mut app,w,1500,TouchPhase::Moved,end.x,end.y);app.update();
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"movement alone cannot issue Close");
        touch(&mut app,w,1500,TouchPhase::Ended,end.x,end.y);
        for _ in 0..6 {app.update();}
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"actual native menu consumer must close");
        assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Playing);
        let seen=app.world().resource::<DefaultMenuTouchSeen>();
        assert_eq!((seen.presses,seen.releases,seen.scrolls),(0,0,0),"owned release cannot activate/scroll inventory underneath");
    }}}
}
#[test]
fn actual_flat_dismissal_cancel_reverse_focus_and_unrelated_ids_preserve_menu_then_fresh_close() {
    for cause in ["cancel","reverse","focus","unknown"] {
        let (mut app,w)=diagonal_flat_scene(Vec2::new(640.,360.),1.,1.,true);
        let p=diagonal_painted(&mut app,false).center();let travel=360.-26.-p.y;
        touch(&mut app,w,1510,TouchPhase::Started,p.x,p.y);app.update();
        let end=p+Vec2::new(-55.,travel*0.8);
        touch(&mut app,w,1510,TouchPhase::Moved,end.x,end.y);app.update();
        if cause=="unknown" {touch(&mut app,w,1511,TouchPhase::Ended,p.x,p.y);app.update();}
        if cause=="focus" {app.world_mut().get_mut::<Window>(w).unwrap().focused=false;app.world_mut().write_message(WindowFocused {window:w,focused:false});}
        let end=if cause=="reverse" {p+Vec2::new(-10.,travel*0.1)} else {end};
        touch(&mut app,w,1510,if cause=="cancel" {TouchPhase::Canceled} else {TouchPhase::Ended},end.x,end.y);
        for _ in 0..5 {app.update();}
        if cause=="unknown" {assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"unrelated terminal cannot cancel the legitimate owner");continue;}
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"cancel/reverse/focus cannot close");
        if cause=="focus" {app.world_mut().get_mut::<Window>(w).unwrap().focused=true;app.world_mut().write_message(WindowFocused {window:w,focused:true});}
        for _ in 0..4 {app.update();}
        let p=diagonal_painted(&mut app,true).center();let end=p+Vec2::new(55.,(360.-26.-p.y)*0.8);
        touch(&mut app,w,1512,TouchPhase::Started,p.x,p.y);app.update();
        touch(&mut app,w,1512,TouchPhase::Ended,end.x,end.y);for _ in 0..5 {app.update();}
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"fresh contact after real neutral still reaches Close");
    }
}
#[test]
fn requested_flat_gutters_preserve_saved_size_and_default_disabled_canvas_scroll() {
    let (mut app,w)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
    app.init_resource::<nvf_play::pad::PlatformInputGate>();
    pipboy_drag::tests::install_native_close_consumer(&mut app);
    diagonal_resize(&mut app,w,Vec2::new(792.,363.),2.,1.5);
    app.insert_resource(nvf_play::pipboy::touch_presentation::PipBoySize::for_platform(true));for _ in 0..5 {app.update();}
    let saved_size=app.world().resource::<nvf_play::pipboy::touch_presentation::PipBoySize>().ratio();
    let before=diagonal_flat(&mut app);
    let p=before.center();
    for (phase,p) in [(TouchPhase::Started,p),(TouchPhase::Moved,p+Vec2::Y*70.),(TouchPhase::Ended,p+Vec2::Y*70.)] {touch(&mut app,w,1520,phase,p.x,p.y);app.update();}
    for _ in 0..5 {app.update();}
    assert!(app.world().resource::<DefaultMenuTouchSeen>().scrolls>0);
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
    diagonal_switch(&mut app,"touch-pipboy-dismiss-left",true);diagonal_switch(&mut app,"touch-pipboy-dismiss-right",true);
    for _ in 0..5 {app.update();}
    let flat=diagonal_flat(&mut app);let left=diagonal_painted(&mut app,true);let right=diagonal_painted(&mut app,false);
    assert!(flat.min.x>=left.max.x && flat.max.x<=right.min.x);
    diagonal_switch(&mut app,"touch-pipboy-dismiss-left",false);diagonal_switch(&mut app,"touch-pipboy-dismiss-right",false);
    for _ in 0..5 {app.update();}
    assert_eq!(diagonal_flat(&mut app),before,"temporary presentation request cannot change saved/default size");
    assert_eq!(app.world().resource::<nvf_play::pipboy::touch_presentation::PipBoySize>().ratio(),saved_size);
}
#[derive(Resource,Default)]struct LateDismissal(Option<Vec2>);
fn late_dismissal_setting(world:&mut World) {
    let Some(at)=world.resource_mut::<LateDismissal>().0.take() else {return;};
    assert!(world.resource::<nvf_play::pipboy::data::touch_map::TouchMap>().projected_point(at).is_some(),"actual old publisher is present before the late change");
    diagonal_switch_world(world,"touch-pipboy-dismiss-left");
}
fn diagonal_switch_world(world:&mut World,key:&str) {
    let row=world.resource::<nvf_play::settings::Settings>().index(key).unwrap();
    assert_eq!(nvf_play::settings::flip(world,row),Some(true));
}
#[test]
fn late_registered_dismissal_setting_refuses_old_flat_map_batch_then_fresh_marker_remains_usable() {
    let (mut app,w)=map_parent_scene(false);
    let p=map_parent_raw(&mut app,Vec2::new(477.5,350.));
    touch(&mut app,w,1530,TouchPhase::Started,p.x,p.y);app.update();
    app.init_resource::<LateDismissal>().add_systems(Update,late_dismissal_setting.after(nvf_play::pipboy::flat_screen).before(nvf_play::pipboy::data::draw));
    app.world_mut().resource_mut::<LateDismissal>().0=Some(p);
    touch(&mut app,w,1530,TouchPhase::Ended,p.x,p.y);app.update();
    assert!(app.world().resource::<MapParentSeen>().asks.is_empty(),"changed actual flat presentation cannot admit a cached map release");
    for _ in 0..6 {app.update();}
    let p=map_parent_raw(&mut app,Vec2::new(477.5,350.));
    touch(&mut app,w,1531,TouchPhase::Started,p.x,p.y);app.update();
    touch(&mut app,w,1531,TouchPhase::Ended,p.x,p.y);app.update();
    assert_eq!(app.world().resource::<MapParentSeen>().asks,[0xF811]);
    assert!(app.world().resource::<MapParentSeen>().go.is_empty());
}

#[test]
fn too_small_flat_view_keeps_its_standard_canvas_when_dismissal_gutters_cannot_fit() {
    let (mut app,w)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
    diagonal_resize(&mut app,w,Vec2::new(192.,192.),1.,1.);
    let before=diagonal_flat(&mut app);
    diagonal_switch(&mut app,"touch-pipboy-dismiss-left",true);
    diagonal_switch(&mut app,"touch-pipboy-dismiss-right",true);
    for _ in 0..6 {app.update();}
    assert_eq!(diagonal_flat(&mut app),before,"requested handles cannot blank an otherwise usable standard menu");
    assert!(before.size().min_element()>0.);
}

// The fallback has no real arm/model. Native inventory is opened only by the
// existing MenuPlugin consuming the parent's actual OpenInventory message.
fn fallback_native_scene(hotbar_enabled:bool)->(App,Entity) {
    use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},asset::{AssetApp,AssetPlugin},
        camera::{ComputedCameraValues,RenderTargetInfo,visibility::VisibilityPlugin},
        ui::{ComputedUiRenderTargetInfo,ui_layout_system,ui_surface::UiSurface,update::propagate_ui_target_cameras}};
    let (mut donor,_)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
    let sim=donor.world_mut().remove_resource::<nvf_play::sim::SimState>().unwrap();
    let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
    app.insert_resource(sim);
    pipboy_drag::tests::install_native_close_consumer(&mut app);
    app.add_plugins((AssetPlugin::default(),bevy::text::TextPlugin,bevy::transform::TransformPlugin))
        .init_asset::<Mesh>().init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
        .add_plugins(VisibilityPlugin)
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
        .init_resource::<UiSurface>().insert_resource(UiScale(1.))
        .add_systems(PostUpdate,(propagate_ui_target_cameras,ui_layout_system).chain())
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
    app.world_mut().spawn((Camera2d,Camera {computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {physical_size:UVec2::new(640,360),scale_factor:1.}),..default()},..default()}));
    diagonal_switch(&mut app,"touch-hotbar",hotbar_enabled);
    diagonal_switch(&mut app,"touch-pipboy-swipe-v1",true);
    // Exercise the real registered setting callback, rather than assuming the
    // fixture helper's initial false value establishes saved Off semantics.
    diagonal_switch(&mut app,"touch-direct-pipboy-v1",true);
    diagonal_switch(&mut app,"touch-direct-pipboy-v1",false);
    for _ in 0..6 {app.update();}
    assert!(!app.world().resource::<pipboy_drag::Options>().direct);
    assert!(app.world().get_resource::<crate::player_body::PlayerBody>().and_then(crate::player_body::PlayerBody::first_person_root).is_none());
    assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Playing);
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
    (app,w)
}
fn fallback_band_point(app:&mut App,w:Entity)->Vec2 {
    let size=app.world().get::<Window>(w).unwrap().resolution.size();
    let bounds={let world=app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<drawer::Handle>>();
        let (node,at)=q.single(world).unwrap();assert!(node.size().min_element()>0.);Rect::from_center_size(at.translation,node.size())};
    assert!(bounds.width()>144.,"the real rendered band must expose new space beyond the old centered handle");
    (16..624).step_by(8).map(|x|Vec2::new(x as f32,bounds.center().y))
        .find(|at|bounds.contains(*at) && (at.x-size.x*0.5).abs()>80.
            && app.world().resource::<Layout>().config.hit_filtered(*at,size,true,false,|_|true).is_none())
        .expect("actual rendered expanded band must leave a new unclaimed start")
}
#[test]
fn actual_fallback_diagonal_release_outside_old_handle_opens_native_inventory_exactly_once() {
    // Establish the unchanged original center/vertical path at the same actual
    // MenuPlugin boundary first, including on the future baseline source.
    {
        let (mut app,w)=fallback_native_scene(false);
        let start={let world=app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<drawer::Handle>>();
            let (node,at)=q.single(world).unwrap();assert!(node.size().min_element()>0.);at.translation};
        touch(&mut app,w,1618,TouchPhase::Started,start.x,start.y);app.update();
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
        touch(&mut app,w,1618,TouchPhase::Ended,start.x,start.y-120.);app.update();app.update();
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.as_ref().is_some_and(|m|m.mode==nvf_play::menu::model::Mode::Inventory),"unchanged vertical center must reach the real opener before testing new diagonal access");
        assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Menu);
    }
    let (mut app,w)=fallback_native_scene(false);
    let start=fallback_band_point(&mut app,w);
    let end=start+Vec2::new(if start.x<320. {55.} else {-55.},-120.);
    let mut requests=bevy::ecs::message::MessageCursor::<nvf_play::menu::OpenInventory>::default();
    touch(&mut app,w,1620,TouchPhase::Started,start.x,start.y);app.update();
    assert_eq!(requests.read(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>()).count(),0);
    assert!(!app.world().resource::<nvf_play::pipboy::gesture::ArmGesture>().captures(),"disabled/absent arm must not be fabricated");
    touch(&mut app,w,1620,TouchPhase::Moved,end.x,end.y);app.update();
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"Move cannot open native inventory");
    assert_eq!(requests.read(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>()).count(),0);
    touch(&mut app,w,1620,TouchPhase::Ended,end.x,end.y);app.update();
    assert_eq!(requests.read(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>()).count(),1);
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.as_ref().is_some_and(|m|m.mode==nvf_play::menu::model::Mode::Inventory),"the actual MenuPlugin consumed the request");
    app.update();assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Menu);
    touch(&mut app,w,1620,TouchPhase::Ended,end.x,end.y);app.update();
    assert_eq!(requests.read(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>()).count(),0,"retired release cannot request another native open");
    assert!(app.world().resource::<TouchState>().contacts.is_empty());
}
#[test]
fn actual_hotbar_capture_excludes_same_batch_fallback_opening_without_losing_fresh_slot_action() {
    let (mut app,w)=fallback_native_scene(true);
    let start=fallback_band_point(&mut app,w);
    // Ordinary fitted slots cannot overlap the bottom band: usable() reserves
    // 64 units plus packing margin. Use the real tray's whole-window capture,
    // which must also exclude a same-batch broad-band swipe.
    let mut hotkeys=bevy::ecs::message::MessageCursor::<nvf_play::weapon_wheel::TouchHotkey>::default();
    let slot=hotbar::test_slot_center(app.world(),0);
    touch(&mut app,w,1630,TouchPhase::Started,slot.x,slot.y);
    touch(&mut app,w,1630,TouchPhase::Ended,slot.x,slot.y);app.update();
    assert_eq!(hotkeys.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).map(|m|m.0).collect::<Vec<_>>(),[0],"actual fresh slot route is live");
    for _ in 0..4 {app.update();} // finish the actual slot release drain before a fresh Pins contact
    let pins=mixed_hotbar_tiles(&mut app).into_iter().find(|(label,_)|label=="Pins").unwrap().1.center();
    touch(&mut app,w,1631,TouchPhase::Started,pins.x,pins.y);
    touch(&mut app,w,1631,TouchPhase::Ended,pins.x,pins.y);app.update();app.update();
    assert!(hotbar::tray_open_collision_fixture(app.world()));
    assert!(hotbar::test_occlusion(app.world()).0,"actual whole-window tray ownership is published before the swipe");
    let mut requests=bevy::ecs::message::MessageCursor::<nvf_play::menu::OpenInventory>::default();
    let end=start+Vec2::new(if start.x<320. {55.} else {-55.},-120.);
    touch(&mut app,w,1632,TouchPhase::Started,start.x,start.y);
    touch(&mut app,w,1632,TouchPhase::Ended,end.x,end.y);app.update();
    assert_eq!(requests.read(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>()).count(),0,"captured hotbar UI must win before fallback Drawer sees Started");
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
    assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Playing);
    assert!(hotkeys.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).next().is_none(),"outside release cannot also use a slot");
}
#[test]
fn held_fallback_drag_retires_on_side_inset_change_then_new_safe_start_opens_native_inventory() {
    let (mut app,w)=fallback_native_scene(false);let start=fallback_band_point(&mut app,w);
    touch(&mut app,w,1640,TouchPhase::Started,start.x,start.y);app.update();
    touch(&mut app,w,1640,TouchPhase::Moved,start.x+40.,start.y-80.);app.update();
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
    app.world_mut().resource_mut::<nvf_play::touch_safe_area::SafeInsets>().left=32.;
    touch(&mut app,w,1640,TouchPhase::Ended,start.x+55.,start.y-120.);app.update();
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"a new left inset cannot inherit an old opening start");
    for _ in 0..3 {app.update();}
    let start=fallback_band_point(&mut app,w);let end=start+Vec2::new(if start.x<320. {55.} else {-55.},-120.);
    touch(&mut app,w,1641,TouchPhase::Started,start.x,start.y);app.update();
    touch(&mut app,w,1641,TouchPhase::Ended,end.x,end.y);app.update();app.update();
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.as_ref().is_some_and(|m|m.mode==nvf_play::menu::model::Mode::Inventory));
    assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Menu);
}
