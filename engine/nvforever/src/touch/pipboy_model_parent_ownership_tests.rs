// Sole-parent raw input -> real posed projection/dispatch -> real UI focus.
// Existing model_control_fixture supplies projection and the actual PipBoy.apply
// method. It does not establish first-person wear/rig or native XML acceptance.
mod model_parent_ownership {
use super::*;
use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},asset::{AssetApp,AssetPlugin},camera::{ComputedCameraValues,RenderTargetInfo,CameraProjection,visibility::VisibilityPlugin},ecs::system::RunSystemOnce,
    ui::{ComputedUiRenderTargetInfo,UiStack,ui_surface::UiSurface,update::propagate_ui_target_cameras}};
use nvf_play::{pipboy::{Page,PipBoy},ui_pointer::{Button,PointerState,UiPointerPlugin,TouchUiCapture}};

#[derive(Component)]struct Underlay;
#[derive(Resource,Default)]struct Seen {presses:usize,releases:usize,down:bool}
fn observe(q:Query<&PointerState,With<Underlay>>,mut seen:ResMut<Seen>) {
    let down=q.single().is_ok_and(|p|*p==PointerState::Pressed);
    seen.presses+=usize::from(down&&!seen.down);seen.releases+=usize::from(!down&&seen.down);seen.down=down;
}
fn idle(app:&mut App,n:usize) {for _ in 0..n {app.update();}}
fn edge(app:&mut App,window:Entity,id:u64,phase:TouchPhase,p:Vec2) {touch(app,window,id,phase,p.x,p.y);app.update();}
fn settings_row(app:&App,key:&str)->usize {app.world().resource::<nvf_play::settings::Settings>().index(key).unwrap()}
fn primary(app:&mut App)->Entity {let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<PrimaryWindow>>();q.single(world).unwrap()}

fn inventory(app:&mut App) {
    use fnv_sim::{FormId,forms::{ActorBase,BaseData,BaseForm,Item,ItemKind}};
    let mut forms=fnv_sim::FormStore::default();
    for i in 0..24 {
        let id=FormId(0xFA700+i);let item=Item {kind:ItemKind::Aid,value:Some(1),money:None,weight:Some(0.5),slots:None,projectile:None,health:None,
            non_playable:false,cant_drop:false,armor_class:default(),power_armor:false,armor_dr:None,armor_dt:None,weapon_damage:None,weight_mods:[0.;3],drawable:false,
            not_for_npcs:false,thrown:false,quest_item:false,icon:None,placed_count:None,teaches:None,cant_be_taken:false,modulates_voice:false,weapon_traits:None};
        forms.sigs.insert(id,*b"ALCH");forms.bases.insert(id,BaseForm {id,sig:*b"ALCH",edid:None,name:Some(format!("Authored aid{i:02}")),script:None,prompt:None,voice_filter:false,data:BaseData::Item(item)});
    }
    forms.sigs.insert(FormId::PLAYER_BASE,*b"NPC_");forms.bases.insert(FormId::PLAYER_BASE,BaseForm {id:FormId::PLAYER_BASE,sig:*b"NPC_",edid:None,name:None,script:None,prompt:None,voice_filter:false,data:BaseData::Actor(ActorBase {entries:vec![],attributes:None})});
    let forms=Box::leak(Box::new(forms));let refs=Box::leak(Box::new(fnv_sim::RefTable::default()));
    let mut sim=nvf_play::sim::SimState {forms,refs,world:fnv_sim::World::new(forms,refs,fnv_sim::SimConfig::default())};
    for i in 0..24 {sim.world.add_item(FormId::PLAYER_REF,FormId(0xFA700+i),3).unwrap();}
    let mut menu=nvf_play::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();
    menu.apply(&mut sim.world,nvf_play::menu::model::Command::SetCategory(nvf_play::menu::model::Side::Player,nvf_play::menu::model::Category::Aid));
    app.insert_resource(sim).insert_resource(nvf_play::menu::ActiveMenu(Some(menu)))
        .init_resource::<nvf_play::pipboy::scrollbar::ItemsScroll>().init_resource::<nvf_play::pipboy::frame::Kit>()
        .init_resource::<nvf_play::pad::PadGlyphs>().init_resource::<nvf_play::pipboy::repair::PipRepair>()
        .init_resource::<nvf_play::pipboy::item_mod::PipItemMod>().init_resource::<nvf_play::pipboy::hotkey_panel::HotkeyHold>()
        .init_resource::<nvf_play::hotkeys::Hotkeys>();
    app.world_mut().resource_mut::<PipBoy>().tabs[Page::Items.index()]=2;
}
struct Fixture {app:App,window:Entity,underlay:Entity,button:Vec2,knob:Vec2}
fn scene() -> Fixture {
    let mut app=crate::pipboy_arm::tests::model_control_fixture();
    app.add_plugins((MinimalPlugins,InputPlugin,StatesPlugin,AssetPlugin::default(),bevy::text::TextPlugin,bevy::transform::TransformPlugin))
        .insert_state(nvf_play::PlayState::Menu).insert_resource(Bindings::default())
        .insert_resource(nvf_play::settings::SettingsFile(None))
        .init_asset::<Mesh>().init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>().init_asset::<Image>()
        .add_plugins((VisibilityPlugin,UiPointerPlugin,
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate)))
        // Production PipBoyPlugin installs this consumer of projected raw points.
        .init_resource::<nvf_play::pipboy::PointerMoved>()
        .add_plugins(nvf_play::pipboy::touch_scroll::TouchPointerPlugin)
        .init_resource::<UiSurface>().init_resource::<UiScale>().init_resource::<UiStack>()
        .add_systems(PostUpdate,(propagate_ui_target_cameras,bevy::ui::ui_layout_system).chain())
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(bevy::ui::ui_layout_system))
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(bevy::ui::ui_layout_system))
        .add_message::<AppLifecycle>().add_message::<WindowFocused>()
        .add_plugins(TouchControlsPlugin).init_resource::<Seen>().add_systems(Update,observe);
    let window=primary(&mut app);app.world_mut().get_mut::<Window>(window).unwrap().resolution=bevy::window::WindowResolution::new(1000,1000);
    {
        let world=app.world_mut();let mut q=world.query_filtered::<&mut Camera,With<nvf_play::PlayerCamera>>();let mut c=q.single_mut(world).unwrap();
        c.computed.clip_from_view=PerspectiveProjection {aspect_ratio:1.0,..default()}.get_clip_from_view();
        c.computed.target_info=Some(RenderTargetInfo {physical_size:UVec2::splat(1000),scale_factor:1.0});
    }
    let ui=app.world_mut().spawn((Camera2d,Camera {computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {physical_size:UVec2::splat(1000),scale_factor:1.0}),..default()},..default()},bevy::ui::IsDefaultUiCamera)).id();
    let mesh={let world=app.world_mut();let mut q=world.query::<(&crate::pipboy_arm::PipBoyPart,&crate::pipboy_arm::PartMesh)>();q.iter(world).find(|(p,_)|matches!(p,crate::pipboy_arm::PipBoyPart::Button(_))).unwrap().1.clone()};
    app.world_mut().spawn((crate::pipboy_arm::PipBoyPart::ScrollKnob,mesh,GlobalTransform::from(Transform::from_xyz(-3.3,-0.45,-10.).with_scale(Vec3::splat(0.2))),Visibility::Visible,InheritedVisibility::VISIBLE));
    inventory(&mut app);idle(&mut app,4);
    app.world_mut().resource_mut::<Layout>().path=None;
    let geometry=app.world().resource::<pipboy_model::Geometry>();
    let button=geometry.buttons.iter().find(|b|b.page!=Page::Items && app.world().resource::<Layout>().config.hit_filtered(b.bounds.center(),Vec2::splat(1000.),false,false,|_|true).is_none()).unwrap().bounds.center();
    let knob=geometry.knob.unwrap().center();
    assert!(app.world().resource::<Layout>().config.hit_filtered(knob,Vec2::splat(1000.),false,false,|_|true).is_none());
    let underlay=app.world_mut().spawn((Underlay,Button,UiTargetCamera(ui),Node {position_type:PositionType::Absolute,left:px(button.x-48.),top:px(button.y-48.),width:px(96.),height:px(96.),..default()})).id();
    idle(&mut app,4);
    let node=app.world().get::<ComputedNode>(underlay).unwrap();assert!(node.size().abs_diff_eq(Vec2::splat(96.),0.01));
    let mut stack=app.world_mut().resource_mut::<UiStack>();stack.uinodes=vec![underlay];stack.partition.clear();stack.partition.push(0..1);
    Fixture {app,window,underlay,button,knob}
}
fn place_underlay(f:&mut Fixture,p:Vec2) {{let mut n=f.app.world_mut().get_mut::<Node>(f.underlay).unwrap();n.left=px(p.x-48.);n.top=px(p.y-48.);}idle(&mut f.app,2);}
fn free_point(f:&Fixture)->Vec2 {
    let g=f.app.world().resource::<pipboy_model::Geometry>();let config=&f.app.world().resource::<Layout>().config;
    [Vec2::new(500.,550.),Vec2::new(500.,600.),Vec2::new(950.,500.)].into_iter().find(|p|
        !g.screen.is_some_and(|r|r.contains(*p))&&!g.buttons.iter().any(|b|b.bounds.contains(*p))&&!g.knob.is_some_and(|r|r.contains(*p))
        &&config.hit_filtered(*p,Vec2::splat(1000.),false,false,|_|true).is_none()).unwrap()
}
fn first(f:&Fixture)->Option<usize> {f.app.world().resource::<nvf_play::pipboy::scrollbar::ItemsScroll>().position()}

#[test]
fn actual_parent_model_page_masks_real_ui_focus_through_release_and_neutral_preserves_hardware() {
    let mut f=scene();let p=f.button;
    let selected=f.app.world().resource::<pipboy_model::Geometry>().buttons.iter().find(|b|b.bounds.contains(p)).unwrap().page;
    let graphics=settings_row(&f.app,"touch-pipboy-button-graphics");assert_eq!(nvf_play::settings::flip(f.app.world_mut(),graphics),Some(false));
    edge(&mut f.app,f.window,1100,TouchPhase::Started,p);
    assert_eq!(f.app.world().resource::<PipBoy>().page,Page::Items);assert_eq!(f.app.world().resource::<Seen>().presses,0);
    assert_eq!(*f.app.world().get::<PointerState>(f.underlay).unwrap(),PointerState::None);
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1100));
    edge(&mut f.app,f.window,1100,TouchPhase::Ended,p);
    assert_eq!(f.app.world().resource::<PipBoy>().page,selected);assert_eq!(f.app.world().resource::<Seen>().presses,0);
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1100),"successful terminal edge remains masked through actual UI focus");
    idle(&mut f.app,3);assert!(f.app.world().resource::<TouchUiCapture>().allows(1100));
    edge(&mut f.app,f.window,1101,TouchPhase::Started,p);
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1101));
    f.app.world_mut().get_mut::<Window>(f.window).unwrap().set_cursor_position(Some(p));
    raw_key(&mut f.app,f.window,KeyCode::F12,ButtonState::Pressed);
    f.app.world_mut().write_message(MouseButtonInput {window:f.window,button:MouseButton::Left,state:ButtonState::Pressed});f.app.update();
    assert_eq!(f.app.world().resource::<Seen>().presses,1);assert!(f.app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    assert!(f.app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1101),"raw mouse still works while the interrupted real finger stays masked");
    raw_key(&mut f.app,f.window,KeyCode::F12,ButtonState::Released);
    f.app.world_mut().write_message(MouseButtonInput {window:f.window,button:MouseButton::Left,state:ButtonState::Released});f.app.update();
    edge(&mut f.app,f.window,1101,TouchPhase::Canceled,p);idle(&mut f.app,3);
    f.app.world_mut().get_mut::<Window>(f.window).unwrap().set_cursor_position(None);
    let free=free_point(&f);place_underlay(&mut f,free);
    edge(&mut f.app,f.window,1100,TouchPhase::Started,free);
    assert_eq!(f.app.world().resource::<Seen>().presses,2,"real neutral restores unrelated UI using the same ID");
    edge(&mut f.app,f.window,1100,TouchPhase::Ended,free);
}

#[test]
fn actual_parent_model_wheel_off_on_keeps_old_tail_masked_and_fresh_drag_scrolls_only_the_list() {
    let mut f=scene();let p=f.knob;place_underlay(&mut f,p);
    let body=f.app.world_mut().spawn((Node::default(),nvf_play::pipboy::PageBody {page:Page::Items,tab:2})).id();
    f.app.world_mut().run_system_once(nvf_play::pipboy::items::draw).unwrap();
    assert!(!f.app.world().get::<Children>(body).unwrap().is_empty());assert_eq!(first(&f),Some(0));
    edge(&mut f.app,f.window,1110,TouchPhase::Started,p);edge(&mut f.app,f.window,1110,TouchPhase::Moved,p+Vec2::Y*19.);
    let row=settings_row(&f.app,"touch-pipboy-model-wheel");assert_eq!(nvf_play::settings::flip(f.app.world_mut(),row),Some(false));
    edge(&mut f.app,f.window,1110,TouchPhase::Moved,p+Vec2::Y*60.);
    assert_eq!(nvf_play::settings::flip(f.app.world_mut(),row),Some(true));
    edge(&mut f.app,f.window,1110,TouchPhase::Ended,p+Vec2::Y*80.);
    assert_eq!(first(&f),Some(0));assert_eq!(f.app.world().resource::<Seen>().presses,0);
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1110));
    edge(&mut f.app,f.window,1111,TouchPhase::Started,p);edge(&mut f.app,f.window,1111,TouchPhase::Moved,p+Vec2::Y*40.);
    edge(&mut f.app,f.window,1111,TouchPhase::Ended,p+Vec2::Y*40.);
    assert_eq!(first(&f),Some(2));assert_eq!(f.app.world().resource::<PipBoy>().page,Page::Items);
    assert_eq!(f.app.world().resource::<Seen>().presses,0);
    let menu=f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.as_ref().unwrap();assert_eq!(menu.selected[nvf_play::menu::model::Side::Player as usize],0);
    assert!(f.app.world().resource::<nvf_play::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
    for i in 0..24 {assert_eq!(f.app.world_mut().resource_mut::<nvf_play::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF,fnv_sim::FormId(0xFA700+i)).unwrap(),3);}
}

#[test]
fn actual_parent_replacement_primary_same_id_does_not_inherit_old_model_mask_or_commit() {
    let mut f=scene();let p=f.button;edge(&mut f.app,f.window,1120,TouchPhase::Started,p);
    let old=f.window;let next=f.app.world().get::<Window>(old).unwrap().clone();
    f.app.world_mut().entity_mut(old).remove::<PrimaryWindow>();f.window=f.app.world_mut().spawn((next,PrimaryWindow)).id();
    let free=free_point(&f);place_underlay(&mut f,free);
    // No old End/Cancel/neutral was fabricated: the old real Touches ID is held.
    edge(&mut f.app,f.window,1120,TouchPhase::Started,free);
    assert_eq!(f.app.world().resource::<PipBoy>().page,Page::Items);assert_eq!(f.app.world().resource::<Seen>().presses,1);
    assert!(f.app.world().resource::<TouchUiCapture>().allows(1120),"only actual current-primary ownership masks this window");
    assert!(f.app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().pending_contact(),
        "the unrelated raw contact must enter the actual projected-pointer route");
    edge(&mut f.app,f.window,1120,TouchPhase::Ended,free);idle(&mut f.app,3);
    assert!(!f.app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().pending_contact(),
        "the real pointer driver must retire the unrelated contact before fresh model admission");
    assert!(f.app.world().resource::<pipboy_model::Geometry>().buttons.iter()
        .any(|button| button.page != Page::Items && button.bounds.contains(p)),
        "replacement primary must publish an actual fresh model target");
    edge(&mut f.app,f.window,1121,TouchPhase::Started,p);edge(&mut f.app,f.window,1121,TouchPhase::Ended,p);
    assert_ne!(f.app.world().resource::<PipBoy>().page,Page::Items,"fresh real model tap remains usable on replacement window");
}

#[test]
fn actual_parent_model_claim_budget_refuses_effects_until_real_neutral_without_clearing_raw_input() {
    let mut f=scene();let p=f.button;
    for id in 1140..1174 {touch(&mut f.app,f.window,id,TouchPhase::Started,p.x,p.y);}f.app.update();
    assert_eq!(f.app.world().resource::<PipBoy>().page,Page::Items);assert_eq!(f.app.world().resource::<Seen>().presses,0);
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1173));
    for id in 1140..1174 {touch(&mut f.app,f.window,id,TouchPhase::Canceled,p.x,p.y);}f.app.update();
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1173));
    idle(&mut f.app,3);assert!(f.app.world().resource::<TouchUiCapture>().allows(1173));
    edge(&mut f.app,f.window,1174,TouchPhase::Started,p);edge(&mut f.app,f.window,1174,TouchPhase::Ended,p);
    assert_ne!(f.app.world().resource::<PipBoy>().page,Page::Items);assert_eq!(f.app.world().resource::<Seen>().presses,0);
}

#[test]
fn actual_parent_model_cancel_and_duplicate_end_keep_ui_quarantined_then_fresh_ui_recovers() {
    let mut f=scene();let p=f.button;
    edge(&mut f.app,f.window,1180,TouchPhase::Started,p);
    edge(&mut f.app,f.window,1180,TouchPhase::Canceled,p);
    assert_eq!(f.app.world().resource::<PipBoy>().page,Page::Items);assert_eq!(f.app.world().resource::<Seen>().presses,0);
    assert!(!f.app.world().resource::<TouchUiCapture>().allows(1180));
    edge(&mut f.app,f.window,1180,TouchPhase::Ended,p);
    assert_eq!(f.app.world().resource::<PipBoy>().page,Page::Items);assert_eq!(f.app.world().resource::<Seen>().presses,0);
    idle(&mut f.app,3);let free=free_point(&f);place_underlay(&mut f,free);
    edge(&mut f.app,f.window,1180,TouchPhase::Started,free);
    assert_eq!(f.app.world().resource::<Seen>().presses,1);assert!(f.app.world().resource::<TouchUiCapture>().allows(1180));
    edge(&mut f.app,f.window,1180,TouchPhase::Ended,free);
}
}
