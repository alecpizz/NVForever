//! CPU consumers of the actual ModelCtx and camera updater; no rendered-pixel proof.
use super::*;
use bevy::asset::{AssetEventSystems, AssetPlugin};
use bevy::camera::{CameraPlugin, CameraUpdateSystems};
use bevy::mesh::MeshPlugin;
use bevy::window::{ExitCondition, WindowPlugin};
use nvf_play::dialogue_device::Stamp;

fn words(out: &mut Vec<u8>, values: &[u32]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}
fn floats(out: &mut Vec<u8>, values: &[f32]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}
fn av_object(name: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out,&[name,0,u32::MAX,14]);
    floats(&mut out,&[0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.,1.,1.]);
    words(&mut out,&[0,u32::MAX]);
    out
}

/// Authored NIF through the owning fixture's existing schema/VFS; a real glass
/// mesh, a larger case behind it, and a raised power switch over one glass corner.
fn terminal_nif() -> Vec<u8> {
    let mut root = av_object(0);
    words(&mut root,&[4,1,3,5,7,0]);
    let mut blocks = vec![("NiNode",root)];
    for (name,bounds,y,uv) in [
        (1,[-5.2,-4.0,5.2,4.0],0.05,[1.,1.]),
        (2,[-4.8,-3.6,4.8,3.6],0.0,[960./1350.,720./960.]),
        (3,[4.1,-3.4,4.5,-3.0],-0.10,[1.,1.]),
        (4,[-4.8,-3.6,4.8,3.6],-0.05,[960./1350.,720./960.]),
    ] {
        let mut shape = av_object(name);
        words(&mut shape,&[(blocks.len()+1) as u32,u32::MAX,0,u32::MAX]);
        shape.push(0);
        let mut data = Vec::new(); words(&mut data,&[0]);
        data.extend(4u16.to_le_bytes()); data.extend([0,0,1]);
        let [left,bottom,right,top] = bounds;
        floats(&mut data,&[left,y,top,right,y,top,left,y,bottom,right,y,bottom]);
        data.extend(1u16.to_le_bytes()); data.push(0);
        floats(&mut data,&[0.,0.,0.,8.]); data.push(0);
        floats(&mut data,&[0.,0.,uv[0],0.,0.,uv[1],uv[0],uv[1]]);
        data.extend(0u16.to_le_bytes()); words(&mut data,&[u32::MAX]);
        for v in [2u16,1,4] { data.extend(v.to_le_bytes()); }
        data.push(1); for v in [0u16,1,2,3] { data.extend(v.to_le_bytes()); }
        blocks.push(("NiTriStrips",shape)); blocks.push(("NiTriStripsData",data));
    }
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out,&[0x1402_0007]); out.push(1);
    words(&mut out,&[11,blocks.len() as u32,34]); out.extend([1,0,1,0,1,0]);
    let mut types = Vec::new();
    for (kind,_) in &blocks { if !types.contains(kind) { types.push(*kind); } }
    out.extend((types.len() as u16).to_le_bytes());
    for name in &types { words(&mut out,&[name.len() as u32]); out.extend(name.as_bytes()); }
    for (kind,_) in &blocks { out.extend((types.iter().position(|t| t==kind).unwrap() as u16).to_le_bytes()); }
    for (_,bytes) in &blocks { words(&mut out,&[bytes.len() as u32]); }
    let names = ["Root","TerminalInterface01:0","screen:0","PowerButton:0","glare:0"];
    words(&mut out,&[names.len() as u32,names.iter().map(|s| s.len()).max().unwrap() as u32]);
    for name in names { words(&mut out,&[name.len() as u32]); out.extend(name.as_bytes()); }
    words(&mut out,&[0]); for (_,bytes) in blocks { out.extend(bytes); }
    words(&mut out,&[1,0]); out
}

fn fixture(width: u32, density: f32) -> (App, crate::outfits::tests::TestFiles) {
    let (mut app,files) = crate::outfits::tests::asset_fixture();
    let path = files.path().join("meshes/terminals");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("terminalinterface01.nif"),terminal_nif()).unwrap();
    // Reopen the actual VFS after adding the authored file, retaining its schema.
    let schema = app.world().resource::<nvf_assets::GameData>().schema.clone();
    app.insert_resource(nvf_assets::GameData::new(
        Arc::new(fnv_vfs::Vfs::open(files.path(),&[]).unwrap()),schema,
    ));
    app.add_plugins((MinimalPlugins,AssetPlugin::default(),bevy::image::ImagePlugin::default(),
        MeshPlugin,CameraPlugin,bevy::transform::TransformPlugin,bevy::input::InputPlugin,
        WindowPlugin {primary_window:Some(Window {resolution:bevy::window::WindowResolution::new(
            (width as f32*density) as u32,(360.*density) as u32).with_scale_factor_override(density),..default()}),
            exit_condition:ExitCondition::DontExit,..default()}))
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<bevy::render::texture::ManualTextureViews>()
        .init_resource::<nvf_play::bindings::Bindings>()
        .init_resource::<nvf_play::terminal::TerminalScreen>()
        .init_resource::<nvf_play::terminal::TerminalPointer>()
        .init_resource::<nvf_play::terminal::PowerAid>()
        .add_message::<nvf_play::terminal::TerminalInput>()
        .add_systems(PostStartup,bevy::render::camera::camera_system.in_set(CameraUpdateSystems))
        .add_systems(PostUpdate,bevy::render::camera::camera_system.in_set(CameraUpdateSystems)
            .before(AssetEventSystems).before(bevy::camera::visibility::update_frusta))
        .add_plugins((super::super::TerminalViewPlugin,DialogueTerminalPlugin));
    let window = {let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<PrimaryWindow>>();q.single(world).unwrap()};
    let owner = app.world_mut().spawn_empty().id();
    let target = nvf_play::terminal::TARGET;
    let mut image = Image::new_fill(Extent3d {width:target.x,height:target.y,depth_or_array_layers:1},
        bevy::render::render_resource::TextureDimension::D2,&[0,0,0,255],
        bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb,
        bevy::asset::RenderAssetUsages::default());
    image.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app.world_mut().spawn((Camera2d,Camera {order:-4,..default()},
        bevy::camera::RenderTarget::Image(bevy::camera::ImageRenderTarget::from(image.clone())))).id();
    app.insert_resource(Request {stamp:Stamp {owner,generation:1,window,kind:DeviceKind::Terminal,
        display:Rect::new(10.25,4.5,width as f32*density-10.5,355.*density),density},
        image,camera,target,canvas:Rect::from_corners(Vec2::ZERO,nvf_play::terminal::screen::CANVAS)});
    (app,files)
}

#[test]
fn actual_model_and_camera_publish_bidirectional_glass_without_housing_hits_or_shrunken_targets() {
    for width in [640,800] { for density in [1.,2.] {
        let (mut app,_files) = fixture(width,density);
        assert!(app.world().get_resource::<DeviceView>().is_none(),"no synthetic camera readiness");
        app.update(); app.update();
        let request = app.world().resource::<Request>();
        let view = app.world().resource::<DeviceView>();
        assert!(view.matches(request));
        for point in [Vec2::new(120.,100.),Vec2::new(480.,360.),Vec2::new(700.,500.)] {
            let window = view.window_at(request,point).expect("real glass point projects");
            assert!(view.canvas_at(request,window).unwrap().distance(point)*view.scale<0.5,
                "real project/unproject round trip stays within half a displayed pixel");
            for logical in [17.,48.] {
                let length = view.canvas_length(request,logical).unwrap();
                let end = view.window_at(request,point+Vec2::Y*length).expect("sized span on glass");
                assert!(end.distance(window)>=logical*density-0.1,
                    "projected font-equivalent and target spans retain requested physical size");
            }
        }
        let image = app.world().resource::<Assets<Image>>().get(&view.image).unwrap();
        assert_eq!(image.width(),request.stamp.display.width().ceil() as u32);
        assert_eq!(image.height(),request.stamp.display.height().ceil() as u32);
        assert!(view.canvas_at(request,request.stamp.display.min-Vec2::ONE).is_none());
        // Use the actual CPU glass and camera to locate the raised authored switch;
        // no input rectangle or precomputed camera fields are written by the fixture.
        let instance = app.world().resource::<DialogueMonitor>().active.as_ref().unwrap();
        let camera = app.world().get::<Camera>(instance.camera).unwrap();
        let at = app.world().get::<GlobalTransform>(instance.camera).unwrap();
        let point = Vec2::new(910.,680.);
        let world = instance.glass.at_uv(point/nvf_play::terminal::screen::TEXTURE_UNITS).unwrap();
        let viewport = camera.world_to_viewport(at,world).unwrap();
        let logical = camera.logical_viewport_rect().unwrap();
        let window = request.stamp.display.min+(viewport-logical.min)/logical.size()*request.stamp.display.size();
        assert!(view.canvas_at(request,window).is_none(),"power/housing cannot become dialogue action");
        let power_rect = view.power_button_rect(request).expect("Retro Console publishes power button rect");
        assert!(power_rect.width() >= 48.0 * density - 0.5 && power_rect.height() >= 48.0 * density - 0.5,
            "power button target scales with display density");
        assert!(view.power_button_at(request, power_rect.center()), "tapping Retro Console power button hits");
    }}
}

#[test]
fn real_dialogue_model_rebind_and_close_preserve_the_existing_terminal_owner() {
    let (mut app,_files) = fixture(640,1.);
    let request = app.world().resource::<Request>().clone();
    app.insert_resource(nvf_play::terminal::TerminalImage {image:request.image.clone(),camera:request.camera});
    app.world_mut().resource_mut::<nvf_play::terminal::TerminalScreen>().0=Some(nvf_play::terminal::Session {
        reference:fnv_sim::FormId(1),base:fnv_sim::FormId(2),
        page:nvf_play::terminal::Page::LockedOut(["Fixture".into(),"Still open".into()]),
    });
    app.update();app.update();
    let original_camera = app.world().resource::<super::super::Monitor>().camera;
    let original_image = app.world().resource::<nvf_play::terminal::MonitorView>().image.clone();
    let old_camera = app.world().resource::<DeviceView>().camera;
    app.world_mut().resource_mut::<Request>().stamp.generation+=1;
    app.update();app.update();
    assert!(app.world().get_entity(old_camera).is_err());
    assert_ne!(app.world().resource::<DeviceView>().camera,old_camera);
    assert!(app.world().resource::<DeviceView>().matches(app.world().resource::<Request>()));
    app.world_mut().remove_resource::<Request>();app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    assert!(app.world().get::<Camera>(original_camera).is_some());
    assert_eq!(app.world().resource::<nvf_play::terminal::MonitorView>().image,original_image);
    assert!(matches!(&app.world().resource::<nvf_play::terminal::TerminalScreen>().0.as_ref().unwrap().page,
        nvf_play::terminal::Page::LockedOut(lines) if lines[1]=="Still open"));
    assert_eq!(app.world().resource::<Messages<nvf_play::terminal::TerminalInput>>().len(),0);
}

#[test]
fn uncovered_canvas_or_stale_window_density_retires_the_actual_model_publication() {
    let (mut app,_files) = fixture(640,1.);
    app.update();app.update();
    assert!(app.world().get_resource::<DeviceView>().is_some());
    app.world_mut().resource_mut::<Request>().canvas.max.y=800.;
    app.world_mut().resource_mut::<Request>().stamp.generation+=1;
    app.update();app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none(),"incomplete real UV coverage is not readiness");
    app.world_mut().resource_mut::<Request>().canvas.max.y=720.;
    app.world_mut().resource_mut::<Request>().stamp.generation+=1;
    app.update();app.update();
    assert!(app.world().get_resource::<DeviceView>().is_some(),"fresh valid request recovers");
    app.world_mut().resource_mut::<Request>().stamp.density=2.;app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none(),"old window geometry cannot retain a mapper");
}

#[test]
fn folded_glass_cannot_reverse_project_a_response_hidden_by_a_nearer_uv_region() {
    use nvf_play::dialogue_device::ScreenMap;
    let (mut app,_files)=fixture(640,1.0);
    app.update();app.update();
    let request=app.world().resource::<Request>();
    let instance=app.world().resource::<DialogueMonitor>().active.as_ref().unwrap();
    let camera=app.world().get::<Camera>(instance.camera).unwrap();
    let transform=app.world().get::<GlobalTransform>(instance.camera).unwrap();
    let texture=nvf_play::terminal::screen::TEXTURE_UNITS;
    let canvas=[Vec2::new(0.0,0.0),Vec2::new(480.0,0.0),
        Vec2::new(0.0,720.0),Vec2::new(480.0,720.0)];
    let back=canvas.map(|point|instance.glass.at_uv(point/texture).unwrap());
    let center=back.iter().copied().sum::<Vec3>()/4.0;
    let toward=(transform.translation()-center).normalize()*0.1;
    let points=back.into_iter().chain(back.into_iter().map(|point|point+toward)).collect();
    let uvs=canvas.into_iter().chain(canvas.into_iter().map(|point|point+Vec2::new(480.0,0.0)))
        .map(|point|point/texture).collect();
    let folded=Arc::new(crate::pipboy_arm::Posed {points,uvs,
        triangles:vec![[0,1,2],[2,1,3],[4,5,6],[6,5,7]]});
    let (map,_)=GlassMap::new(folded,Vec::new(),camera,transform,request.stamp.display,
        request.canvas,Affine2::from_scale(texture)).expect("two UV halves cover the actual canvas");
    assert!(map.window_at(Vec2::new(200.0,360.0)).is_none(),
        "the nearer different-UV triangle must not be returned as the back response");
    let front=Vec2::new(680.0,360.0);
    let window=map.window_at(front).expect("the visible front response still projects");
    assert!(map.canvas_at(window).unwrap().distance(front)<0.25);
}
