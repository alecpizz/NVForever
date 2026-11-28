// Real flat display + Taffy + parent reader + native menu consumer.
#[test]
fn optional_close_button_works_in_actual_flat_pipboy_when_graphics_are_hidden_without_claiming_canvas_scroll() {
    let (mut app,window)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
    app.init_resource::<nvf_play::pad::PlatformInputGate>();
    pipboy_drag::tests::install_native_close_consumer(&mut app);
    {
        let mut options=app.world_mut().resource_mut::<pipboy_options::Options>();
        options.close=true;options.graphics=false;
    }
    for _ in 0..3 {app.update();}
    // Use the actually laid-out flat canvas and the exact painted close bounds.
    let point={let world=app.world_mut();let mut text=world.query::<(&Text,&ChildOf)>();
        let parent=text.iter(world).find(|(t,_)|t.0=="Close").expect("flat canvas must leave the configured close target unclaimed").1.parent();
        let node=world.get::<Node>(parent).unwrap();
        let (Val::Px(x),Val::Px(y),Val::Px(w),Val::Px(h))=(node.left,node.top,node.width,node.height) else {panic!("actual control uses logical pixels")};
        Vec2::new(x+w*0.5,y+h*0.5)};
    let flat={let world=app.world_mut();let mut nodes=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<nvf_play::pipboy::FlatScreen>>();let (node,at)=nodes.single(world).unwrap();
        Rect::from_center_size(at.translation,node.size())};
    assert!(!flat.contains(point),"close affordance must not steal a canvas gesture");
    let body=flat.center();
    touch(&mut app,window,850,TouchPhase::Started,body.x,body.y);
    touch(&mut app,window,850,TouchPhase::Moved,body.x,body.y+80.);
    touch(&mut app,window,850,TouchPhase::Ended,body.x,body.y+80.);
    for _ in 0..3 {app.update();}
    assert!(app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().pending_contact(),
        "the batched canvas End is projected in Last and still awaits the next pointer-consumer frame");
    app.update();
    assert!(!app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().pending_contact(),
        "the actual projected End must be consumed before a fresh close contact");
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"normal list/canvas scrolling never closes");
    for phase in [TouchPhase::Started,TouchPhase::Ended] {touch(&mut app,window,851,phase,point.x,point.y);app.update();assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(851),"hidden close retains actual UI-focus exclusion");}
    for _ in 0..3 {app.update();}
    assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),nvf_play::PlayState::Playing);
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
}

#[test]
fn dismissal_contact_id_overflow_masks_until_real_touch_neutral_and_fresh_close_still_reaches_native_consumer() {
    let (mut app,window)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
    app.init_resource::<nvf_play::pad::PlatformInputGate>();pipboy_drag::tests::install_native_close_consumer(&mut app);
    {let mut options=app.world_mut().resource_mut::<pipboy_options::Options>();options.close=true;options.graphics=false;}
    for _ in 0..3 {app.update();}
    let point={let world=app.world_mut();let mut q=world.query::<(&Text,&ChildOf)>();let e=q.iter(world).find(|(t,_)|t.0=="Close").unwrap().1.parent();
        let n=world.get::<Node>(e).unwrap();let (Val::Px(x),Val::Px(y),Val::Px(w),Val::Px(h))=(n.left,n.top,n.width,n.height) else {panic!("drawn control geometry")};Vec2::new(x+w/2.,y+h/2.)};
    for id in 860..895 {touch(&mut app,window,id,TouchPhase::Started,point.x,point.y);}
    touch(&mut app,window,860,TouchPhase::Canceled,point.x,point.y);app.update();
    assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(894),"unrecordable ID is masked");
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
    for id in 861..895 {touch(&mut app,window,id,TouchPhase::Ended,point.x,point.y);}
    app.update();assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(894));
    for _ in 0..3 {app.update();}
    assert!(app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(894));
    for phase in [TouchPhase::Started,TouchPhase::Ended] {touch(&mut app,window,900,phase,point.x,point.y);app.update();}
    for _ in 0..3 {app.update();}
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
}
