#[test]
fn identical_projected_button_regions_keep_native_page_dispatch_when_graphics_are_invisible_and_restored() {
    let mut app=scene(1.0);
    for (round,graphics,page) in [(0,true,Page::Stats),(1,false,Page::Data),(2,true,Page::Items)] {
        app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics=graphics;settle(&mut app);
        let geometry=app.world().resource::<Geometry>().clone();let point=bounds(&app,page).center();
        event(&mut app,400+round,TouchPhase::Started,point);
        let contact=app.world().resource::<ModelTouch>().contact.as_ref().unwrap();assert!(!contact.cancelled);
        let mut nodes=app.world_mut().query_filtered::<Entity,With<Visual>>();
        let before=nodes.iter(app.world()).collect::<Vec<_>>();
        // Changing paint mid-contact must not change geometry or retire input.
        app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics = !graphics;app.update();
        assert!(*app.world().resource::<Geometry>()==geometry);
        assert!(app.world().resource::<ModelTouch>().contact.is_some());
        let mut ids=app.world_mut().query_filtered::<Entity,With<Visual>>();
        assert_eq!(ids.iter(app.world()).collect::<Vec<_>>(),before,"paint-only toggle must not despawn input-related presentation nodes");
        let world=app.world_mut();let mut nodes=world.query_filtered::<(&Node,&BackgroundColor),With<Visual>>();
        let nodes=nodes.iter(world).collect::<Vec<_>>();assert_eq!(nodes.len(),geometry.buttons.len());
        assert!(nodes.iter().all(|(_,color)|(color.0.to_linear().alpha>0.0)==!graphics));
        event(&mut app,400+round,TouchPhase::Ended,point);settle(&mut app);
        assert_eq!(app.world().resource::<PipBoy>().page,page);
        assert!(app.world().resource::<ModelTouch>().pending.is_none());
    }
}
#[test]
fn actual_physical_model_wheel_option_retires_fraction_and_preserves_list_state_until_fresh_contact() {
    for graphics in [true,false] {
        let (mut app,_)=item_scroll_scene();crate::touch::pipboy_options::install(&mut app);app.insert_resource(nvf_play::settings::SettingsFile(None));app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics=graphics;
        let point=app.world().resource::<Geometry>().knob.unwrap().center();
        event(&mut app,410,TouchPhase::Started,point);event(&mut app,410,TouchPhase::Moved,point+Vec2::Y*19.);
        assert_eq!(first(&app),Some(0));
        let row=app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-model-wheel").unwrap();
        assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(false));
        assert!(app.world().resource::<ModelTouch>().contact.is_none(),"real setting setter retires wheel ownership immediately");
        event(&mut app,410,TouchPhase::Moved,point+Vec2::Y*40.);
        assert_eq!(first(&app),Some(0));assert!(app.world().resource::<ModelTouch>().contact.is_none());
        assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(true));
        event(&mut app,410,TouchPhase::Moved,point+Vec2::Y*80.);event(&mut app,410,TouchPhase::Ended,point+Vec2::Y*80.);
        assert_eq!(first(&app),Some(0),"retired wheel cannot rearm after reenabling");
        event(&mut app,411,TouchPhase::Started,point);event(&mut app,411,TouchPhase::Moved,point+Vec2::Y*40.);
        event(&mut app,411,TouchPhase::Ended,point+Vec2::Y*40.);assert_eq!(first(&app),Some(2));
        keep_items(&mut app);assert!(page_messages(&app).is_empty());
    }
}
