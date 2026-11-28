//! Authored CPU/App consumers. No original-game asset, rendered-pixel or hardware proof.
use super::*;
use bevy::asset::{AssetEventSystems, AssetPlugin};
use bevy::camera::{CameraPlugin, CameraUpdateSystems};
use bevy::ecs::system::RunSystemOnce;
use bevy::mesh::MeshPlugin;
use bevy::window::{ExitCondition, PrimaryWindow, WindowPlugin};
use nvf_play::dialogue_device::Stamp;

const A: FormId = FormId(0x1001);
const B: FormId = FormId(0x1002);

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

/// Authored NIF through the owning fixture's existing schema/VFS; actual CPU glass
/// mesh, a larger case behind it, and a raised power switch over one glass corner.
fn pipboy_nif() -> Vec<u8> {
    let mut root = av_object(0);
    words(&mut root,&[4,1,3,5,7,0]);
    let mut blocks = vec![("NiNode",root)];
    for (name,bounds,y,uv) in [
        (1,[-5.2,-4.0,5.2,4.0],-0.05,[1.,1.]),
        (2,[-4.8,-3.6,4.8,3.6],0.0,[0.8,0.8]),
        (3,[4.1,-3.4,4.5,-3.0],0.10,[1.,1.]),
        (4,[-4.8,-3.6,4.8,3.6],-0.05,[0.8,0.8]),
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
    let names = ["Root","Housing","pipboyscreen:0","PowerButton:0","glare:0"];
    words(&mut out,&[names.len() as u32,names.iter().map(|s| s.len()).max().unwrap() as u32]);
    for name in names { words(&mut out,&[name.len() as u32]); out.extend(name.as_bytes()); }
    words(&mut out,&[0]); for (_,bytes) in blocks { out.extend(bytes); }
    words(&mut out,&[1,0]); out
}

#[derive(Resource, Default)]
struct InputViews(Vec<Option<Entity>>);
fn input_view(view: Option<Res<DeviceView>>, mut seen: ResMut<InputViews>) {
    seen.0.push(view.as_deref().map(|v|v.camera));
}

fn fixture() -> (App, crate::outfits::tests::TestFiles) {
    let (mut app,files) = crate::outfits::tests::asset_fixture();
    for name in ["dialogue_pip_a.nif","dialogue_pip_b.nif"] {
        std::fs::write(files.path().join("meshes").join(name),pipboy_nif()).unwrap();
    }
    let schema = app.world().resource::<GameData>().schema.clone();
    app.insert_resource(GameData::new(Arc::new(fnv_vfs::Vfs::open(files.path(),&[]).unwrap()),schema));
    {
        let mut world = app.world_mut().resource_mut::<ActorWorld>();
        let mut player = world.actors.npcs[&crate::outfits::tests::OTHER].clone();
        player.id=FormId::PLAYER_BASE;
        player.skeleton=Some("skeleton.nif".into());
        world.actors.npcs.insert(FormId::PLAYER_BASE,player);
        for (id,name) in [(A,"dialogue_pip_a.nif"),(B,"dialogue_pip_b.nif")] {
            let mut armor=world.actors.armors[&FormId(1)].clone();
            armor.slots=slot::PIPBOY; armor.addons.clear();
            armor.worn=[Some(name.into()),Some(name.into())];
            world.actors.armors.insert(id,armor);
        }
    }
    app.add_plugins((MinimalPlugins,AssetPlugin::default(),bevy::image::ImagePlugin::default(),
        MeshPlugin,CameraPlugin,bevy::transform::TransformPlugin,
        WindowPlugin {primary_window:Some(Window {resolution:bevy::window::WindowResolution::new(640,360),..default()}),
            exit_condition:ExitCondition::DontExit,..default()}))
        .init_resource::<bevy::render::texture::ManualTextureViews>()
        .init_resource::<InputViews>()
        .add_systems(Update,input_view.in_set(nvf_play::PlaySet::Scripts))
        .add_systems(PostStartup,bevy::render::camera::camera_system.in_set(CameraUpdateSystems))
        .add_systems(PostUpdate,bevy::render::camera::camera_system.in_set(CameraUpdateSystems)
            .before(AssetEventSystems).before(bevy::camera::visibility::update_frusta));
    app.configure_sets(Update,(nvf_play::PlaySet::Scripts,nvf_play::PlaySet::Ui,nvf_play::PlaySet::Sync).chain());
    install(&mut app);
    let window={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<PrimaryWindow>>();q.single(world).unwrap()};
    let owner=app.world_mut().spawn_empty().id();
    let target=nvf_play::pipboy::TARGET;
    let mut image=Image::new_fill(Extent3d {width:target.x,height:target.y,depth_or_array_layers:1},
        TextureDimension::D2,&[0,0,0,255],TextureFormat::Bgra8UnormSrgb,RenderAssetUsages::default());
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let image=app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera=app.world_mut().spawn((Camera2d,Camera {order:-4,..default()},
        RenderTarget::Image(ImageRenderTarget::from(image.clone())))).id();
    app.insert_resource(Request {stamp:Stamp {owner,generation:1,window,kind:DeviceKind::PipBoy,
        display:Rect::new(10.25,4.5,629.5,355.),density:1.},image,camera,target,
        canvas:Rect::from_corners(Vec2::ZERO,Vec2::new(1024.,768.))});
    app.insert_resource(nvf_play::script::fixtures::over_plugins(&[]));
    set_body(&mut app,&[A]);
    (app,files)
}

fn set_body(app:&mut App,worn:&[FormId]) {
    let worn=worn.to_vec();
    app.world_mut().run_system_once(move |mut commands:Commands,world:Res<ActorWorld>,
        mut parts:ResMut<nvf_assets::actor::ActorParts>,mut models:ModelCtx,
        script:Res<nvf_play::script::ScriptState>| {
        let parent=commands.spawn((Transform::default(),Visibility::default())).id();
        let look=script.rt.as_ref().unwrap().game.player.look.as_ref().map(nvf_play::creator::actor_look);
        let mut ctx=nvf_assets::actor::ActorCtx {cache:&mut parts,models:&mut models};
        let body=PlayerBody::dialogue_fixture_body(&mut commands,parent,&world,&mut ctx,&worn,look.as_ref()).unwrap();
        commands.insert_resource(body);
    }).unwrap();
}

fn ready(app:&mut App) -> (Entity,Handle<Image>) {
    app.update();app.update();
    let request=app.world().resource::<Request>();
    let view=app.world().resource::<DeviceView>();
    assert!(view.matches(request));
    let point=request.canvas.center();
    let window=view.window_at(request,point).expect("actual CPU glass projects its center");
    assert!(view.canvas_at(request,window).unwrap().distance(point)*view.scale<0.5);
    (view.camera,view.image.clone())
}

#[test]
fn actual_worn_replacement_and_empty_body_retire_old_view_and_owned_assets_before_input() {
    let (mut app,_files)=fixture(); let (old_camera,old_image)=ready(&mut app);
    let request=app.world().resource::<Request>().clone();
    set_body(&mut app,&[B]); app.update();
    assert_eq!(app.world().resource::<InputViews>().0.last(),Some(&None));
    assert!(app.world().get_entity(old_camera).is_err());
    assert!(app.world().resource::<Assets<Image>>().get(&old_image).is_none());
    let (new_camera,new_image)=ready(&mut app);
    assert_ne!(old_camera,new_camera); assert_ne!(old_image,new_image);
    assert!(same(&request,app.world().resource::<Request>()));
    set_body(&mut app,&[]); app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    assert!(app.world().resource::<Assets<Image>>().get(&new_image).is_none());
    set_body(&mut app,&[A]); let (restored,_) = ready(&mut app);
    // Same worn ID, actual resolver input changes to a different source model.
    app.world_mut().resource_mut::<ActorWorld>().actors.armors.get_mut(&A).unwrap().worn =
        [Some("dialogue_pip_b.nif".into()),Some("dialogue_pip_b.nif".into())];
    app.update();
    assert_eq!(app.world().resource::<InputViews>().0.last(),Some(&None));
    let (rebound,owned_image)=ready(&mut app);assert_ne!(restored,rebound);
    let body_roots=app.world().resource::<PlayerBody>().roots();
    app.world_mut().remove_resource::<Request>();app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    assert!(app.world().get_entity(rebound).is_err());
    assert!(app.world().resource::<Assets<Image>>().get(&owned_image).is_none());
    assert!(app.world().resource::<Assets<Image>>().get(&request.image).is_some(),"content remains owned by Core");
    for root in body_roots {assert!(app.world().get_entity(root).is_ok(),"gameplay body survives dialogue close");}
}

#[test]
fn actual_creator_look_change_and_removal_rebind_same_request_before_input() {
    let (mut app,_files)=fixture(); let (first,_)=ready(&mut app);
    let look=fnv_game::chargen::Look {race:crate::outfits::tests::OTHER,female:true,
        hair:None,hair_color:None,eyes:None,head_parts:vec![],fggs:vec![],fgga:vec![],fgts:vec![]};
    {
        let mut script=app.world_mut().resource_mut::<nvf_play::script::ScriptState>();
        fnv_game::chargen::apply_look(&mut script.rt.as_mut().unwrap().game,look);
    }
    app.update();
    assert_eq!(app.world().resource::<InputViews>().0.last(),Some(&None));
    let (second,_)=ready(&mut app); assert_ne!(first,second);
    app.world_mut().resource_mut::<nvf_play::script::ScriptState>().rt.as_mut().unwrap().game.player.look=None;
    app.update();
    assert_eq!(app.world().resource::<InputViews>().0.last(),Some(&None));
    let (third,_)=ready(&mut app); assert_ne!(second,third);
}

#[test]
fn actual_camera_activation_and_window_focus_recover_without_reloading_model_or_changing_request() {
    let (mut app,_files)=fixture();let (camera,image)=ready(&mut app);
    let request=app.world().resource::<Request>().clone();
    app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=false;app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    assert!(app.world().resource::<Assets<Image>>().get(&image).is_some());
    app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=true;
    assert_eq!(ready(&mut app),(camera,image.clone()));
    app.world_mut().get_mut::<Window>(request.stamp.window).unwrap().focused=false;app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    app.world_mut().get_mut::<Window>(request.stamp.window).unwrap().focused=true;
    assert_eq!(ready(&mut app),(camera,image));
    assert!(same(&request,app.world().resource::<Request>()));
}

#[test]
fn actual_camera_away_mapping_refuses_then_recovers_on_original_transform_without_asset_churn() {
    let (mut app,_files)=fixture();let (camera,image)=ready(&mut app);
    let original=*app.world().get::<Transform>(camera).unwrap();
    app.world_mut().get_mut::<Transform>(camera).unwrap().rotation *= Quat::from_rotation_y(std::f32::consts::PI);
    app.update();app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    for _ in 0..3 {app.update();assert!(app.world().get_resource::<DeviceView>().is_none());}
    assert!(app.world().resource::<Assets<Image>>().get(&image).is_some());
    *app.world_mut().get_mut::<Transform>(camera).unwrap()=original;
    assert_eq!(ready(&mut app),(camera,image));
}

#[cfg(feature="dialogue-model-capture")]
#[test]
fn actual_capture_worn_fallback_never_overrides_a_present_empty_player_body() {
    let (mut app,_files)=fixture(); ready(&mut app);
    app.insert_resource(crate::dialogue_capture::CaptureWorn(vec![A]));
    app.insert_resource(PlayerBody::default());app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
    app.world_mut().remove_resource::<PlayerBody>();ready(&mut app);
    app.world_mut().resource_mut::<crate::dialogue_capture::CaptureWorn>().0=vec![B];app.update();
    assert_eq!(app.world().resource::<InputViews>().0.last(),Some(&None));ready(&mut app);
    app.insert_resource(PlayerBody::default());app.update();
    assert!(app.world().get_resource::<DeviceView>().is_none());
}

#[test]
fn pipboy_skips_light_effect_and_frames_full_bezel_opaque_without_clipping() {
    let (mut app, files) = fixture();
    // Write a Pip-Boy NIF with asymmetric bottom STATS/ITEMS/DATA housing,
    // protruding front bezel/power button, and a huge PipboyLightEffect:0 cone
    // (as in vanilla pipboyarm.nif); prepare() must ignore the light cone,
    // frame every bezel vertex inside the viewport with margin (no top/bottom/
    // right/near clipping), keep screen_material opaque (pipboy_screen == false),
    // and expose PowerButton:0 as hittable.
    let mut root = av_object(0);
    words(&mut root, &[5, 1, 3, 5, 7, 9, 0]);
    let mut blocks = vec![("NiNode", root)];
    for (name, bounds, y, uv) in [
        // Asymmetric housing extending further below the glass (STATS/ITEMS/DATA)
        // behind the glass plane, plus protruding PowerButton:0 in front of glass (+y).
        (1, [-6.6, -5.6, 6.4, 4.4], -0.05, [1., 1.]),
        (2, [-4.8, -3.6, 4.8, 3.6], 0.0, [0.8, 0.8]),
        (3, [5.1, -4.6, 5.9, -3.8], 0.45, [1., 1.]),
        (4, [-4.8, -3.6, 4.8, 3.6], -0.05, [0.8, 0.8]),
        (5, [-80.0, -80.0, 80.0, 80.0], 2.0, [1., 1.]),
    ] {
        let mut shape = av_object(name);
        words(&mut shape, &[(blocks.len() + 1) as u32, u32::MAX, 0, u32::MAX]);
        shape.push(0);
        let mut data = Vec::new(); words(&mut data, &[0]);
        data.extend(4u16.to_le_bytes()); data.extend([0, 0, 1]);
        let [left, bottom, right, top] = bounds;
        floats(&mut data, &[left, y, top, right, y, top, left, y, bottom, right, y, bottom]);
        data.extend(1u16.to_le_bytes()); data.push(0);
        floats(&mut data, &[0., 0., 0., 8.]); data.push(0);
        floats(&mut data, &[0., 0., uv[0], 0., 0., uv[1], uv[0], uv[1]]);
        data.extend(0u16.to_le_bytes()); words(&mut data, &[u32::MAX]);
        for v in [2u16, 1, 4] { data.extend(v.to_le_bytes()); }
        data.push(1); for v in [0u16, 1, 2, 3] { data.extend(v.to_le_bytes()); }
        blocks.push(("NiTriStrips", shape)); blocks.push(("NiTriStripsData", data));
    }
    let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut nif, &[0x1402_0007]); nif.push(1);
    words(&mut nif, &[11, blocks.len() as u32, 34]); nif.extend([1, 0, 1, 0, 1, 0]);
    let mut types = Vec::new();
    for (kind, _) in &blocks { if !types.contains(kind) { types.push(*kind); } }
    nif.extend((types.len() as u16).to_le_bytes());
    for name in &types { words(&mut nif, &[name.len() as u32]); nif.extend(name.as_bytes()); }
    for (kind, _) in &blocks { nif.extend((types.iter().position(|t| t == kind).unwrap() as u16).to_le_bytes()); }
    for (_, bytes) in &blocks { words(&mut nif, &[bytes.len() as u32]); }
    let names = ["Root", "Housing", "pipboyscreen:0", "PowerButton:0", "glare:0", "PipboyLightEffect:0"];
    words(&mut nif, &[names.len() as u32, names.iter().map(|s| s.len()).max().unwrap() as u32]);
    for name in names { words(&mut nif, &[name.len() as u32]); nif.extend(name.as_bytes()); }
    words(&mut nif, &[0]); for (_, bytes) in blocks { out_extend(&mut nif, bytes); }
    words(&mut nif, &[1, 0]);
    std::fs::write(files.path().join("meshes").join("dialogue_pip_a.nif"), nif).unwrap();
    let schema = app.world().resource::<GameData>().schema.clone();
    app.insert_resource(GameData::new(Arc::new(fnv_vfs::Vfs::open(files.path(), &[]).unwrap()), schema));
    *app.world_mut().resource_mut::<ModelCache>() = ModelCache::default();
    let (camera_entity, _) = ready(&mut app);
    let request = app.world().resource::<Request>();
    let view = app.world().resource::<DeviceView>();

    // Opaque CRT screen material: pipboy_screen must be false so alpha = 1.0 (not 0.25).
    let scene = app.world().resource::<Owned>().scene.as_ref().unwrap();
    let screen_mat = app.world().resource::<Assets<NifMaterial>>().get(&scene.screen_material).unwrap();
    assert!(!screen_mat.extension.pipboy_screen, "offscreen dialogue Pip-Boy screen must not write 0.25 SCREEN_MARK alpha");
    assert!(screen_mat.base.unlit);
    assert_eq!(screen_mat.base.alpha_mode, AlphaMode::Opaque);

    // Camera isolation: dedicated layer (distinct from creator_view::STUDIO_LAYER) and Msaa::Sample4.
    assert_ne!(LAYER, crate::creator_view::STUDIO_LAYER);
    assert_eq!(app.world().get::<RenderLayers>(camera_entity), Some(&RenderLayers::layer(LAYER)));
    assert_eq!(app.world().get::<Msaa>(camera_entity), Some(&Msaa::Sample4));

    // Exact frustum framing: every vertex of Housing, Screen, and PowerButton must project
    // strictly inside the viewport with margin and stay safely in front of the near plane.
    let cam = app.world().get::<Camera>(camera_entity).unwrap();
    let cam_gt = app.world().get::<GlobalTransform>(camera_entity).unwrap();
    let near = match app.world().get::<Projection>(camera_entity).unwrap() {
        Projection::Perspective(p) => p.near,
        _ => panic!("expected perspective projection"),
    };
    let cam_inv = cam_gt.affine().inverse();
    for shape in std::iter::once(&scene.glass).chain(&scene.blockers) {
        for &pt in &shape.points {
            let view_pos = cam_inv.transform_point3(pt);
            assert!(-view_pos.z > near * 1.5, "vertex clipped/too close to near plane: depth={} near={near}", -view_pos.z);
            let ndc = cam.world_to_ndc(cam_gt, pt).expect("bezel vertex must project inside frustum");
            assert!(ndc.x.abs() <= 0.98 && ndc.y.abs() <= 0.98, "bezel vertex clipped viewport edge: ndc={ndc:?}");
        }
    }

    // Readable glass height on realistic proportions (> 60% of 350px display height).
    let top_left = view.window_at(request, request.canvas.min + Vec2::splat(2.0)).unwrap();
    let bottom_right = view.window_at(request, request.canvas.max - Vec2::splat(2.0)).unwrap();
    assert!((bottom_right.y - top_left.y).abs() > 210.0, "Wrist Terminal glass height {}", (bottom_right.y - top_left.y).abs());

    // Power button is exposed and clickable outside the glass.
    let power_rect = view.power_button_rect(request).expect("power button rect published");
    assert!(view.power_button_at(request, power_rect.center()));
}

fn out_extend(dst: &mut Vec<u8>, bytes: Vec<u8>) {
    dst.extend(bytes);
}

#[test]
fn dialogue_materials_isolate_screen_opacity_and_additive_glows_from_world_passes() {
    let mut source = NifMaterial::default();
    source.base.alpha_mode = AlphaMode::Add;
    source.extension.first_person = true;
    source.extension.refract = true;
    source.extension.shade_light = true;
    source.extension.soft = Vec4::new(4.0, 0.0, 0.0, 0.0);

    let screen = dialogue_screen_material(&source, Handle::default());
    assert!(!screen.extension.pipboy_screen);
    assert!(!screen.extension.first_person);
    assert!(!screen.extension.refract);
    assert!(!screen.extension.shade_light);
    assert_eq!(screen.extension.soft, Vec4::ZERO);
    assert_eq!(screen.base.alpha_mode, AlphaMode::Opaque);
    assert!(screen.base.unlit);

    let glow = dialogue_accessory_material(&source).expect("additive/first-person shape gets isolated copy");
    assert!(glow.extension.add_linear, "additive Pip-Boy glows must not read view_transmission_texture");
    assert!(!glow.extension.first_person);
    assert!(!glow.extension.refract);
    assert!(!glow.extension.shade_light);
    assert_eq!(glow.extension.soft, Vec4::ZERO);

    // Ordinary opaque housing material needs no copy.
    assert!(dialogue_accessory_material(&NifMaterial::default()).is_none());
}
