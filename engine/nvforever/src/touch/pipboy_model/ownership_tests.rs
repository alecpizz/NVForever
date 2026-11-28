// Actual model dispatch late-window controls; existing scene has real InputPlugin.
use safety_regressions::{bounds, page_messages};
#[test]
fn late_actual_primary_replacement_and_density_change_refuse_captured_model_button() {
    for replacement in [false,true] {
        let mut app=scene(1.0);let point=bounds(&app,Page::Stats).center();
        event(&mut app,501,TouchPhase::Started,point);
        #[derive(Resource)]struct Change(bool,bool);
        fn change(mut commands:Commands,mut once:ResMut<Change>,mut windows:Query<(Entity,&mut Window),With<bevy::window::PrimaryWindow>>) {
            if !std::mem::take(&mut once.0) {return;}
            let (entity,mut window)=windows.single_mut().unwrap();
            if once.1 {
                let new_window=window.clone();
                commands.entity(entity).remove::<bevy::window::PrimaryWindow>();
                commands.spawn((new_window,bevy::window::PrimaryWindow));
            } else {
                window.resolution=bevy::window::WindowResolution::new(2560,1920);
                window.resolution.set_scale_factor_override(Some(2.0));
            }
        }
        app.insert_resource(Change(true,replacement))
            .add_systems(Update,change.after(nvf_play::pipboy::PointerSet).before(dispatch));
        event(&mut app,501,TouchPhase::Ended,point);
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);
        assert!(page_messages(&app).is_empty(),"late actual window identity/physical metadata refuses the old request");
        assert!(app.world().resource::<ModelTouch>().contact.is_none());
        // Restore ordinary density/camera metadata; a genuinely fresh contact works.
        let world=app.world_mut();let mut windows=world.query_filtered::<&mut Window,With<bevy::window::PrimaryWindow>>();
        let mut window=windows.single_mut(world).unwrap();window.resolution=bevy::window::WindowResolution::new(1280,960);
        let mut cameras=world.query_filtered::<&mut Camera,With<nvf_play::PlayerCamera>>();
        cameras.single_mut(world).unwrap().computed.target_info=Some(bevy::camera::RenderTargetInfo {physical_size:UVec2::new(1280,960),scale_factor:1.0});
        settle(&mut app);let point=bounds(&app,Page::Stats).center();
        event(&mut app,502,TouchPhase::Started,point);event(&mut app,502,TouchPhase::Ended,point);
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Stats);
        assert_eq!(page_messages(&app),[PipBoyInput::Page(Page::Stats)]);
    }
}

#[test]
fn actual_model_publication_and_late_page_consumer_require_the_presented_window_but_accept_aliases() {
    use bevy::camera::RenderTarget;
    use bevy::window::WindowRef;
    #[derive(Resource)]struct Retarget(Option<RenderTarget>);
    fn retarget(mut next:ResMut<Retarget>,mut cameras:Query<&mut RenderTarget,With<nvf_play::PlayerCamera>>) {
        if let Some(target)=next.0.take() {*cameras.single_mut().unwrap()=target;}
    }
    for (explicit,late) in [(false,false),(false,true),(true,false),(true,true)] {
        let mut app=scene(1.0);
        let world=app.world_mut();let mut windows=world.query_filtered::<Entity,With<bevy::window::PrimaryWindow>>();let primary=windows.single(world).unwrap();
        let mut cameras=world.query_filtered::<Entity,With<nvf_play::PlayerCamera>>();let camera=cameras.single(world).unwrap();
        let initial=RenderTarget::Window(if explicit {WindowRef::Entity(primary)} else {WindowRef::Primary});
        world.entity_mut(camera).insert(initial);
        world.init_resource::<Assets<Image>>();
        let image=world.resource_mut::<Assets<Image>>().add(Image::new_target_texture(1280,960,bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,None));
        assert_eq!(world.resource::<Assets<Image>>().get(&image).unwrap().size(),UVec2::new(1280,960));
        settle(&mut app);let point=bounds(&app,Page::Stats).center();
        let computed=app.world().get::<Camera>(camera).unwrap().computed.clone();
        let offscreen=RenderTarget::Image(image.into());
        app.insert_resource(Retarget(None)).add_systems(Update,retarget.after(nvf_play::pipboy::PointerSet).before(dispatch));
        if late {
            event(&mut app,510,TouchPhase::Started,point);
            assert!(app.world().resource::<ModelTouch>().contact.is_some());
            app.world_mut().resource_mut::<Retarget>().0=Some(offscreen);
            event(&mut app,510,TouchPhase::Ended,point);
        } else {
            app.world_mut().entity_mut(camera).insert(offscreen);app.update();
            let geometry=app.world().resource::<Geometry>();
            assert!(geometry.buttons.is_empty() && geometry.knob.is_none() && geometry.screen.is_none(),"offscreen camera cannot publish direct model targets");
            event(&mut app,510,TouchPhase::Started,point);event(&mut app,510,TouchPhase::Ended,point);
        }
        assert_eq!(app.world().get::<Camera>(camera).unwrap().computed.clip_from_view,computed.clip_from_view,"same projection does not establish presented ownership");
        assert_eq!(app.world().get::<Camera>(camera).unwrap().physical_target_size(),Some(UVec2::new(1280,960)));
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Items);assert!(page_messages(&app).is_empty());
        assert!(app.world().resource::<ModelTouch>().contact.is_none());
        // Change between the two equivalent window aliases while a fresh contact
        // is held. This must not invalidate the normal presented-world route.
        app.world_mut().entity_mut(camera).insert(RenderTarget::Window(WindowRef::Primary));settle(&mut app);
        let point=bounds(&app,Page::Stats).center();event(&mut app,511,TouchPhase::Started,point);
        app.world_mut().entity_mut(camera).insert(RenderTarget::Window(WindowRef::Entity(primary)));
        event(&mut app,511,TouchPhase::Ended,point);
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Stats);assert_eq!(page_messages(&app),[PipBoyInput::Page(Page::Stats)]);
    }
}
