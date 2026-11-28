// Exact production projector publication, actual posed CPU meshes/camera, no map action injection.
mod map_projection_controls {
    use super::*;
    use nvf_play::pipboy::data::touch_map::TouchMap;
    fn frame(app:&mut App) {app.update();}
    fn point(app:&mut App,uv:Vec2)->Vec2 {
        let world=app.world_mut();let mut parts=world.query::<(&PipBoyPart,&PartMesh,&GlobalTransform)>();
        let (_,mesh,global)=parts.iter(world).find(|(part,_,_)|**part==PipBoyPart::Screen).unwrap();
        let shape=pointer_pose(&mesh.0,None,global.to_matrix()).unwrap();let point=shape.at_uv(uv).unwrap();
        let mut cameras=world.query_filtered::<(&Camera,&GlobalTransform),With<PlayerCamera>>();let (camera,at)=cameras.single(world).unwrap();
        camera.world_to_viewport(at,point).unwrap()
    }
    #[test]
    fn actual_device_map_projector_uses_same_visible_nearest_ray_at_density_and_independent_lens_changes() {
        for density in [1.0,2.0] {for android_size in [false,true] {
            let mut app=model_control_fixture();app.init_resource::<TouchMap>().init_resource::<crate::touch::pipboy_model::Geometry>();
            {let mut pip=app.world_mut().resource_mut::<PipBoy>();pip.page=Page::Data;pip.frame=Frame::Device;pip.tabs[Page::Data.index()]=1;}
            let sizing=nvf_play::pipboy::touch_presentation::PipBoySize::for_platform(android_size);
            let lens=sizing.device_fov(45.0).to_radians();app.insert_resource(sizing);
            {let world=app.world_mut();let mut windows=world.query::<&mut Window>();let mut window=windows.single_mut(world).unwrap();
                window.resolution=bevy::window::WindowResolution::new((1280.*density) as u32,(960.*density) as u32);window.resolution.set_scale_factor_override(Some(density));}
            {let world=app.world_mut();let mut cameras=world.query::<&mut Camera>();let mut camera=cameras.single_mut(world).unwrap();
                camera.computed.clip_from_view=bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection{fov:lens,aspect_ratio:1280.0/960.0,..default()});
                camera.computed.target_info=Some(bevy::camera::RenderTargetInfo{physical_size:UVec2::new((1280.*density) as u32,(960.*density) as u32),scale_factor:density});}
            // Keep the complete authored mesh inside both real lens frusta;
            // clipped source geometry is deliberately unavailable in production.
            let screen={let world=app.world_mut();let mut q=world.query::<(Entity,&PipBoyPart)>();q.iter(world).find_map(|(e,p)|(*p==PipBoyPart::Screen).then_some(e)).unwrap()};
            app.world_mut().entity_mut(screen).insert(GlobalTransform::from(Transform::from_xyz(0.,1.,-10.).with_scale(Vec3::new(2.,1.5,1.))));
            frame(&mut app);let uv=Vec2::splat(0.4);let raw=point(&mut app,uv);
            let want=canvas_point(uv).unwrap();let actual=app.world().resource::<TouchMap>().projected_point(raw).unwrap();
            assert!((actual-want).length()<0.01,"published mapper must match the actual posed screen UV at each lens/density");
            let camera={let world=app.world_mut();let mut q=world.query_filtered::<(&Camera,&GlobalTransform),With<PlayerCamera>>();let (c,t)=q.single(world).unwrap();(c.clone(),*t)};
            let ray=camera.0.viewport_to_world(&camera.1,raw).unwrap();let front=ray.origin+Vec3::from(ray.direction)*5.0;
            let button={let world=app.world_mut();let mut q=world.query::<(Entity,&PipBoyPart)>();q.iter(world).find_map(|(e,p)|(*p==PipBoyPart::Button(Page::Stats)).then_some(e)).unwrap()};
            app.world_mut().entity_mut(button).insert(GlobalTransform::from(Transform::from_translation(front).with_scale(Vec3::splat(0.3))));frame(&mut app);
            assert!(app.world().resource::<TouchMap>().projected_point(raw).is_none(),"a real nearer visible button blocks the screen, never a guessed rectangular crop");
            app.world_mut().entity_mut(button).insert(Visibility::Hidden);frame(&mut app);
            assert!((app.world().resource::<TouchMap>().projected_point(raw).unwrap()-want).length()<0.01);
            let camera={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<PlayerCamera>>();q.single(world).unwrap()};
            app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=false;frame(&mut app);
            assert!(app.world().resource::<TouchMap>().projected_point(raw).is_none(),"inactive source cannot retain a latent map projector");
        }}
    }
}

pub(crate) fn attach_map_pointer_fixture(app:&mut App) {
    let mut authored=model_control_fixture();
    let rows={let world=authored.world_mut();let mut q=world.query::<(&PipBoyPart,&PartMesh,&GlobalTransform,&Visibility,&InheritedVisibility)>();
        q.iter(world).map(|(p,m,g,v,i)|(*p,PartMesh(m.0.clone()),*g,*v,*i)).collect::<Vec<_>>()};
    for row in rows {app.world_mut().spawn(row);}
    let mut camera=Camera::default();camera.computed.clip_from_view=bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection{fov:45f32.to_radians(),aspect_ratio:1.,..default()});
    camera.computed.target_info=Some(bevy::camera::RenderTargetInfo{physical_size:UVec2::splat(1000),scale_factor:1.});
    app.world_mut().spawn((camera,GlobalTransform::IDENTITY,PlayerCamera));
    app.init_resource::<Assets<SkinnedMeshInverseBindposes>>().init_resource::<crate::touch::pipboy_model::Geometry>()
        .add_systems(Update,follow_pointer.in_set(nvf_play::pipboy::PointerSet).before(nvf_play::pipboy::data::draw));
    {let mut pip=app.world_mut().resource_mut::<PipBoy>();pip.frame=Frame::Device;}
    app.world_mut().resource_mut::<PipBoyScreen>().on_model=true;
}
pub(crate) fn map_pointer_fixture_raw(app:&mut App,canvas:Vec2)->Vec2 {
    let world=app.world_mut();let mut parts=world.query::<(&PipBoyPart,&PartMesh,&GlobalTransform)>();
    let (_,mesh,global)=parts.iter(world).find(|(p,_,_)|**p==PipBoyPart::Screen).unwrap();
    let shape=pointer_pose(&mesh.0,None,global.to_matrix()).unwrap();let world_point=shape.at_uv(canvas/CANVAS*UV_EXTENT).unwrap();
    let mut q=world.query_filtered::<(&Camera,&GlobalTransform),With<PlayerCamera>>();let (c,g)=q.single(world).unwrap();c.world_to_viewport(g,world_point).unwrap()
}
