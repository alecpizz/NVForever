// R2 controls: actual Bevy/Taffy paint conversion and late source-window identity.
fn install_control_layout(app:&mut App,scale:f32) {
    use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},asset::{AssetApp,AssetPlugin},camera::visibility::VisibilityPlugin,
        ui::{ComputedUiRenderTargetInfo,ui_layout_system,ui_surface::UiSurface,update::propagate_ui_target_cameras}};
    app.add_plugins((AssetPlugin::default(),bevy::text::TextPlugin,bevy::transform::TransformPlugin))
        .init_asset::<Mesh>().init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
        .add_plugins(VisibilityPlugin)
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
        .init_resource::<UiSurface>().insert_resource(UiScale(scale))
        .add_systems(PostUpdate,(propagate_ui_target_cameras,ui_layout_system).chain())
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
        .configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
}
fn painted_control(app:&mut App,label:&str)->(Entity,Rect,Entity) {
    let world=app.world_mut();let mut texts=world.query::<(&Text,&ChildOf)>();
    let parent=texts.iter(world).find(|(t,_)|t.0==label).expect("actual graphic caption").1.parent();
    let node=world.get::<ComputedNode>(parent).unwrap();let transform=world.get::<UiGlobalTransform>(parent).unwrap();
    assert!(node.size().min_element()>0.0 && node.size().is_finite(),"actual Taffy layout must resolve the target");
    let camera=world.get::<ComputedUiTargetCamera>(parent).unwrap().get().unwrap();
    (parent,Rect::from_center_size(transform.translation,node.size()),camera)
}
#[test]
fn actual_taffy_dismiss_and_projected_model_controls_match_raw_input_at_three_ui_scales_with_graphics_hidden_and_restored() {
    for scale in [0.75,1.0,1.5] {
        // Dismissal preview is the same target used by the real native close consumer.
        let mut scene=optional_scene();install_control_layout(&mut scene.app,scale);
        install_native_close_consumer(&mut scene.app);closing_idle(&mut scene,5);
        let expected=scene.app.world().resource::<State>().regions.iter().find(|(t,_)|*t==crate::touch::pipboy_options::Target::Close).unwrap().1;
        let (entity,painted,camera)=painted_control(&mut scene.app,"Close");
        assert!((painted.min-expected.min).abs().max_element()<1.0 && (painted.max-expected.max).abs().max_element()<1.0);
        assert_eq!(camera,scene.camera,"explicit overlay targets the actual full-window camera");
        let row=scene.app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-button-graphics").unwrap();
        assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),row),Some(false));closing_idle(&mut scene,2);
        assert_eq!(painted_control(&mut scene.app,"Close"),(entity,painted,camera),"paint-only hide keeps actual target entity and geometry");
        assert_eq!(scene.app.world().get::<BackgroundColor>(entity).unwrap().0.to_linear().alpha,0.0);
        assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),row),Some(true));closing_idle(&mut scene,2);
        assert_eq!(painted_control(&mut scene.app,"Close"),(entity,painted,camera));
        assert_eq!(nvf_play::settings::flip(scene.app.world_mut(),row),Some(false));closing_idle(&mut scene,2);
        let p=painted.center();closing_edge(&mut scene,950,TouchPhase::Started,p);scene.app.update();
        closing_edge(&mut scene,950,TouchPhase::Ended,p);closing_idle(&mut scene,3);
        assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"raw tap at actual invisible painted bounds reaches native Close");
        // Existing projected bottom buttons use the same window/UiScale conversion.
        let mut scene=closing_scene();install_control_layout(&mut scene.app,scale);closing_idle(&mut scene,5);
        let expected=scene.app.world().resource::<crate::touch::pipboy_model::Geometry>().buttons.iter().find(|b|b.page==nvf_play::pipboy::Page::Stats).unwrap().bounds;
        let (_,painted,camera)=painted_control(&mut scene.app,nvf_play::pipboy::Page::Stats.title());
        assert!((painted.min-expected.min).abs().max_element()<1.0 && (painted.max-expected.max).abs().max_element()<1.0);
        assert_eq!(camera,scene.camera);
        scene.app.world_mut().resource_mut::<crate::touch::pipboy_options::Options>().graphics=false;closing_idle(&mut scene,2);
        let p=painted.center();closing_edge(&mut scene,951,TouchPhase::Started,p);scene.app.update();
        closing_edge(&mut scene,951,TouchPhase::Ended,p);closing_idle(&mut scene,2);
        assert_eq!(scene.app.world().resource::<PipBoy>().page,nvf_play::pipboy::Page::Stats);
    }
}
#[test]
fn actual_native_close_rejects_same_geometry_primary_window_replacement_after_raw_end_then_fresh_new_window_contact_works() {
    #[derive(Resource)]struct Replace {old:Entity,new:Option<Entity>,armed:bool,saw_pending:bool}
    fn replace(mut commands:Commands,mut replace:ResMut<Replace>,windows:Query<&Window>,pending:Res<State>) {
        if !std::mem::take(&mut replace.armed) {return;}
        let request=pending.pending.as_ref().expect("raw End must queue the actual close before source replacement");
        assert_eq!(request.window.entity,replace.old);
        replace.saw_pending=true;
        let old=windows.get(replace.old).unwrap();
        let replacement=commands.spawn((Window {resolution:old.resolution.clone(),focused:old.focused,..default()},bevy::window::PrimaryWindow)).id();
        commands.entity(replace.old).remove::<bevy::window::PrimaryWindow>();replace.new=Some(replacement);
    }
    let mut scene=optional_scene();install_native_close_consumer(&mut scene.app);closing_idle(&mut scene,3);
    scene.app.insert_resource(Replace {old:scene.window,new:None,armed:false,saw_pending:false})
        .add_systems(Update,replace.after(nvf_play::pipboy::PointerSet).before(dispatch_close));
    let p=optional_point(&scene,crate::touch::pipboy_options::Target::Close);
    closing_edge(&mut scene,960,TouchPhase::Started,p);scene.app.update();
    closing_edge(&mut scene,960,TouchPhase::Ended,p);scene.app.world_mut().resource_mut::<Replace>().armed=true;
    scene.app.update();
    assert!(scene.app.world().resource::<Replace>().saw_pending);
    assert!(scene.app.world().resource::<State>().pending.is_none());
    assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"same-size new Primary must not inherit the old window's native close");
    scene.window=scene.app.world().resource::<Replace>().new.unwrap();closing_idle(&mut scene,3);
    assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"denied old close never replays");
    let p=optional_point(&scene,crate::touch::pipboy_options::Target::Close);
    closing_edge(&mut scene,961,TouchPhase::Started,p);scene.app.update();
    closing_edge(&mut scene,961,TouchPhase::Ended,p);closing_idle(&mut scene,3);
    assert!(scene.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),"fresh new-window owner can close through actual native consumer");
}
