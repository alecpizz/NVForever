// Projection/command fixtures. The base explicitly supplies on_model=true; it
// does not establish first-person body wear or show_screen ownership coverage.
mod safety_regressions {
    use super::*;
    use crate::pipboy_arm::{PartMesh,PipBoyPart};
    use bevy::{asset::RenderAssetUsages,ecs::system::RunSystemOnce,mesh::{Indices,PrimitiveTopology,skinning::{SkinnedMesh,SkinnedMeshInverseBindposes}}};
    use std::sync::Arc;
    fn part(app:&mut App,wanted:PipBoyPart)->Entity {
        let world=app.world_mut();let mut parts=world.query::<(Entity,&PipBoyPart)>();
        parts.iter(world).find_map(|(entity,p)|(*p==wanted).then_some(entity)).unwrap()
    }
    fn camera(app:&mut App)->Entity {
        let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<nvf_play::PlayerCamera>>();q.single(world).unwrap()
    }
    pub(super) fn bounds(app:&App,page:Page)->Rect {app.world().resource::<Geometry>().buttons.iter().find(|b|b.page==page).unwrap().bounds}
    fn square()->Mesh {
        let mut mesh=Mesh::new(PrimitiveTopology::TriangleList,RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION,vec![[-1.0_f32,-1.,0.],[1.,-1.,0.],[1.,1.,0.],[-1.,1.,0.]]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0,vec![[0.0_f32,0.],[1.,0.],[1.,1.],[0.,1.]]);
        mesh.insert_indices(Indices::U16(vec![0,1,2,0,2,3]));mesh
    }
    fn knob_pose(rotation:f32)->GlobalTransform {
        // The real fixture camera has a 45-degree, aspect-1 frustum: x=5 at
        // z=-10 is wholly offscreen. Keep the authored knob visibly beside
        // the screen and above the page buttons, including its animated pose.
        GlobalTransform::from(Transform::from_xyz(3.5,-0.5,-10.).with_scale(Vec3::splat(0.35)).with_rotation(Quat::from_rotation_z(rotation)))
    }
    fn knob(app:&mut App)->Entity {
        let entity=app.world_mut().spawn((PipBoyPart::ScrollKnob,PartMesh(Arc::new(square())),
            knob_pose(0.0),Visibility::Visible,InheritedVisibility::VISIBLE)).id();
        settle(app);assert!(app.world().resource::<Geometry>().knob.is_some());
        let camera_entity=camera(app);let world=app.world();
        let camera=world.get::<Camera>(camera_entity).unwrap();let at=world.get::<GlobalTransform>(camera_entity).unwrap();
        let geometry=world.resource::<Geometry>();let viewport=Rect::from_corners(Vec2::ZERO,geometry.size);
        for rotation in [0.0,0.5] {
            let pose=knob_pose(rotation);
            for corner in [Vec3::new(-1.,-1.,0.),Vec3::new(1.,-1.,0.),Vec3::new(1.,1.,0.),Vec3::new(-1.,1.,0.)] {
                let point=camera.world_to_viewport(at,pose.transform_point(corner)).unwrap();
                assert!(viewport.contains(point),"the complete real knob mesh must remain in the actual camera viewport");
            }
        }
        let point=camera.world_to_viewport(at,world.get::<GlobalTransform>(entity).unwrap().translation()).unwrap();
        assert!(geometry.knob.unwrap().contains(point),"the actual projected mesh center owns the knob target");
        assert!(!geometry.on_screen(point),"screen precedence must not steal the knob fixture contact");
        assert!(geometry.target(point).is_none(),"a page button must not steal the knob fixture contact");
        entity
    }
    pub(super) fn page_messages(app:&App)->Vec<PipBoyInput> {
        let mut cursor=bevy::ecs::message::MessageCursor::<PipBoyInput>::default();
        cursor.read(app.world().resource::<Messages<PipBoyInput>>()).copied().collect()
    }
    #[test]
    fn inactive_source_and_late_camera_handoff_cannot_commit_a_model_page() {
        #[derive(Resource,Default)]struct Late(bool);
        fn deactivate(mut late:ResMut<Late>,mut cameras:Query<&mut Camera,With<nvf_play::PlayerCamera>>) {
            if std::mem::take(&mut late.0) {cameras.single_mut().unwrap().is_active=false;}
        }
        for late in [false,true] {
            let mut app=scene(1.0);let point=bounds(&app,Page::Stats).center();let camera=camera(&mut app);
            app.init_resource::<Late>().add_systems(Update,deactivate.after(nvf_play::pipboy::PointerSet).before(dispatch));
            if late {
                event(&mut app,201,TouchPhase::Started,point);
                app.world_mut().resource_mut::<Late>().0=true;
            } else {
                app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=false;app.update();
                assert!(app.world().resource::<Geometry>().buttons.is_empty(),"inactive camera cannot publish direct touch targets");
                event(&mut app,201,TouchPhase::Started,point);
            }
            event(&mut app,201,TouchPhase::Ended,point);settle(&mut app);
            assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);assert!(page_messages(&app).is_empty());
            assert!(app.world().resource::<ModelTouch>().contact.is_none());
            app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=true;settle(&mut app);
            let point=bounds(&app,Page::Stats).center();event(&mut app,202,TouchPhase::Started,point);event(&mut app,202,TouchPhase::Ended,point);
            assert_eq!(app.world().resource::<PipBoy>().page,Page::Stats);assert_eq!(page_messages(&app),[PipBoyInput::Page(Page::Stats)]);
        }
    }
    #[test]
    fn hidden_button_and_hidden_screen_are_unavailable_to_direct_and_generic_touch_projection() {
        let mut app=scene(1.0);let data=part(&mut app,PipBoyPart::Button(Page::Data));let old=bounds(&app,Page::Data).center();
        app.world_mut().entity_mut(data).insert(Visibility::Hidden);app.update();
        assert!(app.world().resource::<Geometry>().buttons.iter().all(|b|b.page!=Page::Data));
        event(&mut app,210,TouchPhase::Started,old);event(&mut app,210,TouchPhase::Ended,old);
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);assert!(page_messages(&app).is_empty());
        // Ask the real PartMesh ray projection to map a point on the actual screen.
        let screen=part(&mut app,PipBoyPart::Screen);
        let tri=app.world().resource::<Geometry>().screen_triangles[0];let point=(tri[0]+tri[1]+tri[2])/3.;
        app.init_resource::<nvf_play::pipboy::touch_items::TouchItems>();
        fn sample(app:&mut App,id:u64,point:Vec2)->Option<Vec2> {
            let input=TouchInput {window:Entity::PLACEHOLDER,id,phase:TouchPhase::Started,position:point,force:None};
            assert!(app.world_mut().resource_mut::<nvf_play::pipboy::touch_items::TouchItems>().event(&input,0));app.update();
            app.world_mut().resource_mut::<nvf_play::pipboy::touch_items::TouchItems>().take_projected().expect("real projection pass must consume the queued sample").1
        }
        assert!(sample(&mut app,211,point).is_some(),"visible source control proves the ray reaches real screen triangles");
        app.world_mut().resource_mut::<nvf_play::pipboy::touch_items::TouchItems>().cancel();
        app.world_mut().entity_mut(screen).insert(Visibility::Hidden);app.update();
        assert!(app.world().resource::<Geometry>().screen.is_none());
        assert!(sample(&mut app,212,point).is_none(),"hidden screen cannot keep receiving generic native menu touches");
    }
    #[test]
    fn incomplete_skin_sources_and_short_attributes_fail_closed_before_projection() {
        for malformed in 0..9 {
            let mut app=scene(1.0);let data=part(&mut app,PipBoyPart::Button(Page::Data));let old=bounds(&app,Page::Data).center();
            let joint=if malformed==1 {app.world_mut().spawn_empty().id()}else{app.world_mut().spawn(GlobalTransform::IDENTITY).id()};
            let bind=if malformed==0 {Handle::default()}else{
                let mut matrix=Mat4::IDENTITY;
                if malformed==8 {matrix.x_axis.w=0.25;}
                app.world_mut().resource_mut::<Assets<SkinnedMeshInverseBindposes>>().add(SkinnedMeshInverseBindposes::from(vec![matrix]))
            };
            let mut mesh=square();
            if malformed!=2 {
                let count=if malformed==3 {1}else{4};
                let index:u16=if malformed==4 {7}else{0};
                mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX,bevy::mesh::VertexAttributeValues::Uint16x4(vec![[index,0,0,0];count]));
                let weight=match malformed {5=>f32::NAN,6=>0.0,7=>0.5,_=>1.0};
                mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT,vec![[weight,0.,0.,0.];4]);
            }
            app.world_mut().entity_mut(data).insert((PartMesh(Arc::new(mesh)),SkinnedMesh {inverse_bindposes:bind,joints:vec![joint]}));
            app.update();assert!(app.world().resource::<Geometry>().buttons.iter().all(|b|b.page!=Page::Data),"malformed source{malformed} must not become rigid or publish an arbitrary target");
            event(&mut app,220,TouchPhase::Started,old);event(&mut app,220,TouchPhase::Ended,old);
            assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);assert!(page_messages(&app).is_empty());
            let fresh=bounds(&app,Page::Stats).center();event(&mut app,221,TouchPhase::Started,fresh);event(&mut app,221,TouchPhase::Ended,fresh);
            assert_eq!(app.world().resource::<PipBoy>().page,Page::Stats,"valid remaining direct button still works in source case{malformed}");
        }
    }
    #[test]
    fn finite_offscreen_rejection_and_real_screen_overlap_precede_enlarged_button_hits() {
        for invalid in [false,true] {
            let mut app=scene(1.0);let data=part(&mut app,PipBoyPart::Button(Page::Data));let old=bounds(&app,Page::Data).center();
            let x=if invalid {f32::NAN}else{1.0e9};app.world_mut().entity_mut(data).insert(GlobalTransform::from_translation(Vec3::new(x,-2.5,-10.)));app.update();
            assert!(app.world().resource::<Geometry>().buttons.iter().all(|b|b.page!=Page::Data));
            event(&mut app,230,TouchPhase::Started,old);event(&mut app,230,TouchPhase::Ended,old);
            assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);assert!(page_messages(&app).is_empty());
        }
        let mut app=scene(1.0);let stats=part(&mut app,PipBoyPart::Button(Page::Stats));
        app.world_mut().entity_mut(stats).insert(GlobalTransform::from(Transform::from_xyz(0.,-0.02,-10.).with_scale(Vec3::splat(0.01))));settle(&mut app);
        let button=bounds(&app,Page::Stats);let point=button.center()-Vec2::Y*10.;
        assert!(button.width()>=48. && button.height()>=48. && button.contains(point));
        assert!(app.world().resource::<Geometry>().on_screen(point),"fixture must overlap enlarged button with actual projected screen triangle");
        event(&mut app,231,TouchPhase::Started,point);assert!(app.world().resource::<ModelTouch>().contact.is_none());
        event(&mut app,231,TouchPhase::Ended,point);assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);assert!(page_messages(&app).is_empty());
    }
    #[test]
    fn unrelated_knob_pose_animation_preserves_the_real_button_contact_stamp() {
        let mut app=scene(1.0);let knob=knob(&mut app);let point=bounds(&app,Page::Data).center();let old_knob=app.world().resource::<Geometry>().knob;
        event(&mut app,240,TouchPhase::Started,point);
        app.world_mut().entity_mut(knob).insert(knob_pose(0.5));app.update();
        assert_ne!(app.world().resource::<Geometry>().knob,old_knob,"fixture must really animate knob projected bounds");
        assert!(app.world().resource::<ModelTouch>().contact.is_some(),"unrelated knob animation is not button ownership replacement");
        event(&mut app,240,TouchPhase::Ended,point);
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Data);assert_eq!(page_messages(&app),[PipBoyInput::Page(Page::Data)]);
    }

    fn item_scroll_scene()->(App,Entity) {
        use fnv_sim::{FormId,forms::{ActorBase,BaseData,BaseForm,Item,ItemKind}};
        let mut app=scene(1.0);let mut forms=fnv_sim::FormStore::default();
        for i in 0..24 {
            let id=FormId(0xF700+i);
            let item=Item {kind:ItemKind::Aid,value:Some(1),money:None,weight:Some(0.5),slots:None,projectile:None,health:None,
                non_playable:false,cant_drop:false,armor_class:default(),power_armor:false,armor_dr:None,armor_dt:None,weapon_damage:None,
                weight_mods:[0.;3],drawable:false,not_for_npcs:false,thrown:false,quest_item:false,icon:None,placed_count:None,teaches:None,
                cant_be_taken:false,modulates_voice:false,weapon_traits:None};
            forms.sigs.insert(id,*b"MISC");forms.bases.insert(id,BaseForm {id,sig:*b"MISC",edid:None,name:Some(format!("Authored aid{i:02}")),script:None,prompt:None,voice_filter:false,data:BaseData::Item(item)});
        }
        forms.sigs.insert(FormId::PLAYER_BASE,*b"MISC");forms.bases.insert(FormId::PLAYER_BASE,BaseForm {id:FormId::PLAYER_BASE,sig:*b"MISC",edid:None,name:None,script:None,prompt:None,voice_filter:false,data:BaseData::Actor(ActorBase {entries:vec![],attributes:None})});
        let forms=Box::leak(Box::new(forms));let refs=Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut sim=nvf_play::sim::SimState {forms,refs,world:fnv_sim::World::new(forms,refs,fnv_sim::SimConfig::default())};
        for i in 0..24 {sim.world.add_item(FormId::PLAYER_REF,FormId(0xF700+i),3).unwrap();}
        let mut model=nvf_play::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();
        model.apply(&mut sim.world,nvf_play::menu::model::Command::SetCategory(nvf_play::menu::model::Side::Player,nvf_play::menu::model::Category::Aid));
        app.insert_resource(sim).insert_resource(nvf_play::menu::ActiveMenu(Some(model)))
            .init_resource::<nvf_play::pipboy::scrollbar::ItemsScroll>().init_resource::<nvf_play::pipboy::frame::Kit>()
            .init_resource::<Assets<Image>>().init_resource::<nvf_play::pad::PadGlyphs>()
            .init_resource::<nvf_play::pipboy::repair::PipRepair>().init_resource::<nvf_play::pipboy::item_mod::PipItemMod>()
            .init_resource::<nvf_play::pipboy::hotkey_panel::HotkeyHold>().init_resource::<nvf_play::hotkeys::Hotkeys>();
        app.world_mut().resource_mut::<PipBoy>().tabs[Page::Items.index()]=2;
        let body=app.world_mut().spawn((Node::default(),nvf_play::pipboy::PageBody {page:Page::Items,tab:2})).id();
        app.world_mut().run_system_once(nvf_play::pipboy::items::draw).unwrap();
        assert!(!app.world().get::<Children>(body).unwrap().is_empty(),"actual ITEMS painter must establish the list/marker view");
        assert_eq!(app.world().resource::<nvf_play::pipboy::scrollbar::ItemsScroll>().position(),Some(0));
        let knob=knob(&mut app);(app,knob)
    }
    fn first(app:&App)->Option<usize> {app.world().resource::<nvf_play::pipboy::scrollbar::ItemsScroll>().position()}
    fn keep_items(app:&mut App) {
        let model=app.world().resource::<nvf_play::menu::ActiveMenu>().0.as_ref().unwrap();
        assert_eq!(model.selected[nvf_play::menu::model::Side::Player as usize],0,"view knob does not select/use a gameplay row");
        assert!(app.world().resource::<nvf_play::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
        for i in 0..24 {assert_eq!(app.world_mut().resource_mut::<nvf_play::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF,fnv_sim::FormId(0xF700+i)).unwrap(),3);}
    }
    #[test]
    fn real_knob_twenty_logical_pixels_per_line_reaches_actual_shared_list_state_despite_knob_animation() {
        for density in [1.0,2.0] {
            let (mut app,knob)=item_scroll_scene();
            if density!=1.0 {
                let world=app.world_mut();let mut q=world.query::<&mut Window>();let mut w=q.single_mut(world).unwrap();
                w.resolution=bevy::window::WindowResolution::new(2560,1920);w.resolution.set_scale_factor_override(Some(2.0));
                let camera=camera(&mut app);app.world_mut().get_mut::<Camera>(camera).unwrap().computed.target_info=Some(bevy::camera::RenderTargetInfo {physical_size:UVec2::new(2560,1920),scale_factor:2.0});settle(&mut app);
            }
            let point=app.world().resource::<Geometry>().knob.unwrap().center();event(&mut app,250,TouchPhase::Started,point);
            app.world_mut().entity_mut(knob).insert(knob_pose(0.5));app.update();
            assert!(app.world().resource::<ModelTouch>().contact.is_some(),"animated knob bounds must not cancel its owner");
            event(&mut app,250,TouchPhase::Moved,point+Vec2::Y*39.0);assert_eq!(first(&app),Some(1));
            event(&mut app,250,TouchPhase::Moved,point+Vec2::Y*40.0);assert_eq!(first(&app),Some(2));
            event(&mut app,250,TouchPhase::Ended,point+Vec2::Y*40.0);settle(&mut app);assert_eq!(first(&app),Some(2));
            assert!(page_messages(&app).is_empty(),"knob uses typed list-scroll requests, never a page or gameplay button");keep_items(&mut app);
        }
    }
    #[test]
    fn late_knob_admission_failure_retires_contact_and_fraction_until_new_started() {
        #[derive(Resource,Default)]struct Late(bool);
        fn capture(mut late:ResMut<Late>,mut capture:ResMut<nvf_play::pipboy::touch_items::OverlayCapture>) {
            if std::mem::take(&mut late.0) {capture.0=true;}
        }
        let (mut app,_)=item_scroll_scene();app.init_resource::<Late>().init_resource::<nvf_play::pipboy::touch_items::OverlayCapture>()
            .add_systems(Update,capture.after(nvf_play::pipboy::PointerSet).before(dispatch));
        let point=app.world().resource::<Geometry>().knob.unwrap().center();event(&mut app,260,TouchPhase::Started,point);
        event(&mut app,260,TouchPhase::Moved,point+Vec2::Y*19.0);assert_eq!(first(&app),Some(0));
        app.world_mut().resource_mut::<Late>().0=true;
        event(&mut app,260,TouchPhase::Moved,point+Vec2::Y*20.0);
        assert_eq!(first(&app),Some(0));assert!(app.world().resource::<ModelTouch>().contact.is_none(),"late denial must retire held ownership, not only discard one delta");
        app.world_mut().resource_mut::<nvf_play::pipboy::touch_items::OverlayCapture>().0=false;
        event(&mut app,260,TouchPhase::Moved,point+Vec2::Y*60.0);event(&mut app,260,TouchPhase::Ended,point+Vec2::Y*60.0);settle(&mut app);
        assert_eq!(first(&app),Some(0),"old held contact/fraction cannot rearm after capture closes");
        event(&mut app,261,TouchPhase::Started,point);event(&mut app,261,TouchPhase::Moved,point+Vec2::Y*20.0);event(&mut app,261,TouchPhase::Ended,point+Vec2::Y*20.0);
        assert_eq!(first(&app),Some(1),"fresh contact works after cancellation");keep_items(&mut app);
    }

    #[test]
    fn closing_region_requires_complete_live_screen_and_visible_control_footprints() {
        fn unclaimed(app:&mut App)->bool {
            app.world_mut().run_system_once(|bridge:Bridge|bridge.closing_unclaimed(Vec2::new(640.,940.))).unwrap()
        }
        let mut app=scene(1.0);assert!(unclaimed(&mut app),"valid scene has a genuinely empty bottom handle region");
        let screen=part(&mut app,PipBoyPart::Screen);let mesh=app.world().get::<PartMesh>(screen).unwrap().clone();
        app.world_mut().entity_mut(screen).remove::<PartMesh>();app.update();
        assert!(app.world().resource::<Geometry>().screen.is_none());
        assert!(!app.world().resource::<Geometry>().buttons.is_empty(),"partial direct button source remains available");
        assert!(!unclaimed(&mut app),"missing screen CPU projection is unknown space, never an arm-close opening");
        app.world_mut().entity_mut(screen).insert(mesh);app.update();assert!(unclaimed(&mut app));
        let button=part(&mut app,PipBoyPart::Button(Page::Data));app.world_mut().entity_mut(button).remove::<PartMesh>();app.update();
        assert!(app.world().resource::<Geometry>().screen.is_some());
        assert!(!unclaimed(&mut app),"one unprojectable visible recognized button invalidates closing completeness too");
    }
    #[test]
    fn queued_real_model_scroll_rechecks_camera_destination_after_dispatch_then_fresh_alias_drag_works() {
        use bevy::{camera::RenderTarget,window::WindowRef,ecs::schedule::ScheduleBuildSettings};
        #[derive(Resource)]struct Retarget(Option<RenderTarget>,bool);
        fn retarget(mut next:ResMut<Retarget>,mut cameras:Query<&mut RenderTarget,With<nvf_play::PlayerCamera>>,state:Res<ModelTouch>) {
            if let Some(target)=next.0.take() {
                assert!(state.admitted && state.contact.is_some(),"the real dispatcher must still have an admitted held owner before the late queued-consumer denial");
                next.1=true;*cameras.single_mut().unwrap()=target;
            }
        }
        let (mut app,_)=item_scroll_scene();let camera=camera(&mut app);
        let world=app.world_mut();let mut windows=world.query_filtered::<Entity,With<bevy::window::PrimaryWindow>>();let primary=windows.single(world).unwrap();
        let image=world.resource_mut::<Assets<Image>>().add(Image::new_target_texture(1280,960,bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,None));
        assert_eq!(world.resource::<Assets<Image>>().get(&image).unwrap().size(),UVec2::new(1280,960));
        // Keep the actual Commands queue until the end of Update, allowing a
        // genuine same-frame destination change after dispatch but before the
        // queued ListScrollRequest consumer. No pending/scroll effect is seeded.
        app.edit_schedule(Update,|schedule| {schedule.set_build_settings(ScheduleBuildSettings {auto_insert_apply_deferred:false,..default()});});
        app.insert_resource(Retarget(None,false)).add_systems(Update,retarget.after(dispatch).before(draw).before(nvf_play::PlaySet::Ui));
        let point=app.world().resource::<Geometry>().knob.unwrap().center();
        event(&mut app,520,TouchPhase::Started,point);
        assert!(app.world().resource::<ModelTouch>().contact.is_some());
        app.world_mut().resource_mut::<Retarget>().0=Some(RenderTarget::Image(image.into()));
        event(&mut app,520,TouchPhase::Moved,point+Vec2::Y*40.);
        assert!(app.world().resource::<Retarget>().1,"the target actually changes after production dispatch, before deferred application");
        assert_eq!(first(&app),Some(0),"the real queued scroll refuses a now-offscreen source");
        assert!(app.world().resource::<ModelTouch>().contact.is_none(),"failed queued source retires only its own live contact");
        assert!(page_messages(&app).is_empty());keep_items(&mut app);
        event(&mut app,520,TouchPhase::Ended,point+Vec2::Y*40.);
        app.world_mut().entity_mut(camera).insert(RenderTarget::Window(WindowRef::Entity(primary)));settle(&mut app);
        let point=app.world().resource::<Geometry>().knob.unwrap().center();
        event(&mut app,521,TouchPhase::Started,point);
        app.world_mut().entity_mut(camera).insert(RenderTarget::Window(WindowRef::Primary));
        event(&mut app,521,TouchPhase::Moved,point+Vec2::Y*40.);event(&mut app,521,TouchPhase::Ended,point+Vec2::Y*40.);
        assert_eq!(first(&app),Some(2),"fresh direct wheel drag accepts equivalent current-primary aliases");
        assert!(page_messages(&app).is_empty());keep_items(&mut app);
    }
    include!("optional_tests.rs");
}
