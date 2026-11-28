//! Raw parent input -> public PipBoyPlugin -> persistent body Animator CPU boundary.
//! Authored decoded clip/rig and camera metadata do not establish GPU readiness.
use super::*;
use fnv_nif::anim::{Channel, Key, KeyKind, Keys, TransformTrack};
use bevy::camera::{CameraProjection, RenderTarget, RenderTargetInfo};
use bevy::input::{InputPlugin, touch::{TouchInput, TouchPhase}};
use bevy::state::app::StatesPlugin;
use bevy::window::{AppLifecycle, PrimaryWindow, WindowFocused, WindowResolution};
use nvf_play::{PlaySet, PlayState};
use nvf_play::pipboy::{PipBoy, PipBoyPlugin, PipBoyScreen, Phase, HOLD_SECONDS};
use nvf_play::pipboy::gesture::ArmGesture;

struct Coupled {
    app: App,
    window: Entity,
    root: Entity,
    bone: Entity,
    screen: Entity,
    _files: crate::outfits::tests::TestFiles,
}

#[derive(Resource, Default)]
struct LastAppliedClip(Option<(f32, Vec3)>, bool);

fn observe_applied_clip(body: Res<PlayerBody>, transforms: Query<&Transform>, mut seen: ResMut<LastAppliedClip>) {
    let rig = body.body.as_ref().unwrap().first.as_ref().unwrap();
    if let Some(layer) = rig.animator.layers.iter().find(|l|l.tag==TAG_PIPBOY) {
        let local = transforms.get(rig.animator.nodes[1]).unwrap().translation;
        seen.0 = Some((layer.time,local));
        if layer.time==0. && local==Vec3::ZERO { seen.1=true; }
    }
}

fn coupled_scene() -> Coupled {
    coupled_scene_with_inertia(false)
}
fn coupled_scene_with_inertia(inertia: bool) -> Coupled {
    let (mut app, files) = crate::outfits::tests::asset_fixture();
    app.add_plugins((MinimalPlugins, InputPlugin, StatesPlugin, bevy::transform::TransformPlugin))
        .init_state::<PlayState>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
        .insert_resource(nvf_play::settings::SettingsFile(None))
        .insert_resource(nvf_play::bindings::Bindings::default())
        .insert_resource(nvf_play::pause::PlayClock::frozen_frame(0.05))
        .init_resource::<nvf_play::pause::Paused>()
        .init_resource::<PlayerControl>()
        .init_resource::<MoveIntent>()
        .init_resource::<BodyView>()
        .init_resource::<PhysState>()
        .init_resource::<WeaponAnim>()
        .init_resource::<PlayerIdles>()
        .init_resource::<crate::script_sequences::PlayerSequences>()
        .init_resource::<crate::furniture::BodyOverride>()
        .init_resource::<nvf_play::furniture::PlayerSeat>()
        .init_resource::<crate::death_cam::DeathCam>()
        .init_resource::<crate::player_knockdown::PlayerKnockdown>()
        .init_resource::<crate::weapon_lower::Ahead>()
        .init_resource::<nvf_play::player::Lunge>()
        .init_resource::<nvf_play::player::ViewDriven>()
        .init_resource::<crate::stealth_field::StealthField>()
        .init_resource::<nvf_play::pad::PlatformInputGate>()
        .init_resource::<nvf_play::pad::PadGlyphs>()
        .init_resource::<nvf_play::hotkeys::Hotkeys>()
        .init_resource::<nvf_play::hitvol::HitVolumes>()
        .init_resource::<LastAppliedClip>()
        .add_message::<AppLifecycle>()
        .add_message::<WindowFocused>()
        .add_message::<nvf_play::loading::TravelRequest>()
        .add_message::<nvf_play::wait::HourPassed>()
        .add_message::<nvf_play::OpenContainer>()
        .add_message::<nvf_play::RefChanged>()
        .add_message::<nvf_play::ItemUsed>()
        .add_message::<nvf_play::pickpocket::OpenPickpocket>()
        .add_message::<nvf_play::pickpocket::Pickpocketed>()
        .add_message::<nvf_play::crime::ItemsTaken>()
        .add_message::<nvf_play::ui_sounds::UiSound>()
        .add_message::<nvf_play::sfx::PlaySound>();
    let empty = crate::outfits::tests::empty_world_data();
    app.insert_resource(PlayWorld(nvf_play::PlayData {
        lo: empty.lo, schema: empty.schema, index: empty.index,
        world: 0, interior: None, eye: [0.; 3], heading_deg: 0., pitch_deg: 0.,
        auto_walk: 0., start_at_cell_spot: false, packages: false,
    })).insert_resource(nvf_play::Location { world: 0, interior: None })
        .insert_resource(Player { heading: 0., pitch: 0., eye_height: 0., feet: [0.; 3],
            body_eye: None, body_view: None, placed: None });
    if inertia {
        app.insert_resource(nvf_play::inertia::MovementInertia::all_on());
        assert!(app.world().resource::<nvf_play::inertia::MovementInertia>().smooth_hands());
    }
    install_wearable(&mut app);
    let window = app.world_mut().spawn((Window {
        resolution: WindowResolution::new(1000, 1000), ..default()
    }, PrimaryWindow)).id();
    let projection = PerspectiveProjection { aspect_ratio: 1., ..default() };
    let mut camera = Camera::default();
    camera.computed.clip_from_view = projection.get_clip_from_view();
    camera.computed.target_info = Some(RenderTargetInfo { physical_size: UVec2::splat(1000), scale_factor: 1. });
    app.world_mut().spawn((camera, Projection::Perspective(projection), Transform::IDENTITY,
        GlobalTransform::IDENTITY, RenderTarget::Window(bevy::window::WindowRef::Primary), nvf_play::PlayerCamera));
    let (root, bone) = install_cached_rig(&mut app);
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-1.,-1.,0.],[1.,-1.,0.],[1.,1.,0.],[-1.,1.,0.]]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32,0.],[1.,0.],[1.,1.],[0.,1.]]);
    mesh.insert_indices(bevy::mesh::Indices::U16(vec![0,1,2,0,2,3]));
    let mesh = Arc::new(mesh);
    let mesh_handle = app.world_mut().resource_mut::<Assets<Mesh>>().add((*mesh).clone());
    let material = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
    let screen = app.world_mut().spawn((crate::pipboy_arm::PipBoyPart::Screen,
        crate::pipboy_arm::PartMesh(mesh), Mesh3d(mesh_handle), MeshMaterial3d(material),
        Transform::from_xyz(0., 0., -10.).with_scale(Vec3::new(2.,1.5,1.)),
        GlobalTransform::default(), ChildOf(bone), Visibility::Visible, InheritedVisibility::VISIBLE)).id();
    app.configure_sets(Update, (PlaySet::Input, PlaySet::Physics, PlaySet::Activation, PlaySet::Scripts, PlaySet::Ui, PlaySet::Sync).chain())
        .configure_sets(Update, nvf_play::player::BodySet.in_set(PlaySet::Physics))
        .add_plugins((crate::touch::TouchControlsPlugin, nvf_play::menu::MenuPlugin, PipBoyPlugin))
        .add_systems(Update, (drive_body,observe_applied_clip).chain().in_set(nvf_play::player::BodySet));
    crate::pipboy_arm::tests::install_coupled_clip_projection(&mut app);
    app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Playing);
    app.update();
    // Configure through the saved-setting callbacks, after all installers register.
    let index = app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-dismiss-side-v2").expect("R4 side setting installed");
    assert!(nvf_play::settings::pick(app.world_mut(), index, "Both"));
    let travel = app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-dismiss-travel-v1").unwrap();
    assert_eq!(nvf_play::settings::set_slider(app.world_mut(),travel,0.375),Some(0.375));
    app.world_mut().write_message(nvf_play::menu::OpenInventory);
    for _ in 0..12 { app.update(); }
    assert_eq!(app.world().resource::<PipBoy>().phase, Phase::Up);
    assert_eq!(app.world().resource::<PipBoy>().clip_time, HOLD_SECONDS);
    assert!(app.world().resource::<PipBoyScreen>().on_model, "actual show_screen admits the driven first rig");
    assert!(app.world().resource::<crate::touch::pipboy_model::Geometry>().screen.is_some(), "actual posed mesh publishes the display");
    // The endpoint witness below is about this contact, not the initial raise.
    app.world_mut().resource_mut::<LastAppliedClip>().1=false;
    Coupled { app, window, root, bone, screen, _files: files }
}

fn install_wearable(app: &mut App) {
    let mut forms = fnv_sim::FormStore::default();
    forms.bases.insert(FormId::PLAYER_BASE, fnv_sim::forms::BaseForm {
        id: FormId::PLAYER_BASE, sig: *b"NPC_", edid: None, name: Some("Authored player".into()),
        script: None, prompt: None, voice_filter: false,
        data: fnv_sim::forms::BaseData::Actor(fnv_sim::forms::ActorBase {entries:vec![],attributes:None}),
    });
    forms.sigs.insert(FormId::PLAYER_BASE,*b"NPC_");
    let id = FormId(0xF201);
    let item = fnv_sim::forms::Item {
        kind: fnv_sim::forms::ItemKind::Armor, value: None, money: None, weight: None,
        slots: Some(1 << 6), projectile: None, health: None, non_playable: false,
        cant_drop: false, armor_class: default(), power_armor: false, armor_dr: None, armor_dt: None,
        weapon_damage: None, weight_mods: [0.; 3], drawable: false, not_for_npcs: false,
        thrown: false, quest_item: false, icon: None, placed_count: None, teaches: None,
        cant_be_taken: false, modulates_voice: false, weapon_traits: None,
    };
    forms.bases.insert(id, fnv_sim::forms::BaseForm { id, sig: *b"ARMO", edid: None,
        name: Some("Authored coupled wrist".into()), script: None, prompt: None,
        voice_filter: false, data: fnv_sim::forms::BaseData::Item(item) });
    forms.sigs.insert(id, *b"ARMO");
    let forms = Box::leak(Box::new(forms));
    let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
    let mut sim = SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) };
    sim.world.add_item(FormId::PLAYER_REF, id, 1).unwrap();
    sim.world.equip(FormId::PLAYER_REF, id).unwrap();
    assert!(nvf_play::pipboy::wears_pipboy(&sim));
    app.insert_resource(sim);
}

fn install_cached_rig(app: &mut App) -> (Entity, Entity) {
    let root = app.world_mut().spawn((Transform::IDENTITY, GlobalTransform::IDENTITY, Visibility::Visible)).id();
    let bone = app.world_mut().spawn((Transform::IDENTITY, GlobalTransform::IDENTITY, ChildOf(root))).id();
    let skel = Arc::new(PreparedSkeleton {
        names: vec![Some("OwnedRoot".into()), Some("OwnedArm".into())],
        parents: vec![None,Some(0)], locals: vec![Transform::IDENTITY;2],
        bind_world: vec![Affine3A::IDENTITY;2], by_name: [("OwnedRoot".into(),0),("OwnedArm".into(),1)].into(),
        bbx_half_width: None,
    });
    let actor = crate::limbs::ActorRig { reference: 0, base: 0, body: 0, skeleton: "Owned".into(),
        skel, nodes: vec![root,bone], base_scale: 1. };
    let third_root = app.world_mut().spawn((Transform::IDENTITY, GlobalTransform::IDENTITY, Visibility::Visible)).id();
    let mut third = actor.clone();
    third.nodes = vec![third_root];
    third.skel = Arc::new(PreparedSkeleton {
        names: vec![Some("OwnedThird".into())],
        parents: vec![None],
        locals: vec![Transform::IDENTITY],
        bind_world: vec![Affine3A::IDENTITY],
        by_name: [("OwnedThird".into(), 0)].into(),
        bbx_half_width: None,
    });
    let mut body = PlayerBody::synthetic_script_models((third_root,third),Some((root,actor)));
    let clip = Arc::new(Clip {
        name: "Authored coupled arm".into(), start: 0., stop: nvf_play::pipboy::CLIP_SECONDS,
        frequency: 1., cycle_type: 2,
        tracks: vec![("OwnedArm".into(),Arc::new(TransformTrack {
            translation: Channel::Keys(Keys { kind: KeyKind::Linear,
                keys: [(0.,-2.),(HOLD_SECONDS,0.)].into_iter().map(|(time,y)| Key {
                    time, value: [0.,y,0.], forward: [0.;3], backward: [0.;3], tbc: [0.;3],
                }).collect() }), rotation: Channel::Static(None), euler: None,
            scale: Channel::Static(None), per_cycle: None,
        }))], priorities: vec![98], skipped: 0, accum_root: None, text_keys: vec![],
        visibility: vec![], emitters: vec![], materials: vec![], free: vec![], free_visibility: vec![], lights: vec![],
    });
    body.body.as_mut().unwrap().first.as_mut().unwrap().clips.insert(ClipKey::PipBoy(false),
        Some(Arc::new(ClipInfo::new("owned/coupled.kf".into(),clip,None))));
    app.insert_resource(body);
    (root,bone)
}

fn raw(f: &mut Coupled, id: u64, phase: TouchPhase, p: Vec2) {
    f.app.world_mut().write_message(TouchInput { window: f.window, id, phase, position: p, force: None });
    f.app.update();
}

fn actual_pose(f: &mut Coupled, expected_clip: f32) -> Vec3 {
    assert!((f.app.world().resource::<PipBoy>().clip_time-expected_clip).abs()<0.0001);
    // Gesture drive writes in Ui after BodySet. The next ordinary body pass
    // consumes that time; no direct animation or deferred-effect driver is used.
    f.app.update();
    let state = f.app.world().resource::<PlayerBody>();
    let rig = state.body.as_ref().unwrap().first.as_ref().unwrap();
    assert_eq!(rig.root,f.root);
    assert_eq!(rig.animator.nodes[1],f.bone);
    let layer = rig.animator.layers.iter().find(|l|l.tag==TAG_PIPBOY).expect("actual body layer");
    assert!((layer.time-expected_clip).abs()<0.0001);
    assert_eq!(layer.rate,0.);
    assert_eq!(rig.animator.locals[1],*f.app.world().get::<Transform>(f.bone).unwrap());
    let fraction = expected_clip/HOLD_SECONDS;
    let expected_y = (-2.+2.*fraction)*pipboy_ease(expected_clip);
    let local = f.app.world().get::<Transform>(f.bone).unwrap().translation;
    assert!((local.y-expected_y).abs()<0.0001,"actual clip interpolation and applied transform: {local:?}");
    assert!(f.app.world().get::<GlobalTransform>(f.screen).unwrap().translation().is_finite());
    local
}

#[test]
fn raw_both_outside_display_sides_lower_reverse_and_cancel_the_same_real_clip() {
    for left in [true,false] {
        let mut f=coupled_scene();
        let screen=f.app.world().resource::<crate::touch::pipboy_model::Geometry>().screen.unwrap();
        let p=Vec2::new(if left {screen.min.x-36.} else {screen.max.x+36.},screen.center().y);
        assert!(!screen.contains(p));
        assert!(Rect::from_corners(Vec2::ZERO,Vec2::splat(1000.)).contains(p));
        raw(&mut f,811,TouchPhase::Started,p);
        assert!(f.app.world().resource::<ArmGesture>().captures());
        raw(&mut f,811,TouchPhase::Moved,p+Vec2::new(40.,60.));
        let mid=f.app.world().resource::<PipBoy>().clip_time;
        assert!((mid-HOLD_SECONDS*(1.-52./112.)).abs()<0.0001,"saved 120-unit travel and 8-unit deadzone reach the real clip");
        assert!(mid>0. && mid<HOLD_SECONDS,"raw displacement must continuously lower the arm");
        let lower=actual_pose(&mut f,mid);
        raw(&mut f,811,TouchPhase::Moved,p+Vec2::new(40.,28.));
        let reversed=f.app.world().resource::<PipBoy>().clip_time;
        assert!((reversed-HOLD_SECONDS*(1.-20./112.)).abs()<0.0001);
        assert!(reversed>mid && reversed<HOLD_SECONDS);
        let back=actual_pose(&mut f,reversed);
        assert_ne!(lower,back);
        assert!(f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
        raw(&mut f,811,TouchPhase::Canceled,p+Vec2::new(40.,28.));
        for _ in 0..24 { if !f.app.world().resource::<ArmGesture>().captures(){break;} f.app.update(); }
        assert!(!f.app.world().resource::<ArmGesture>().captures());
        assert_eq!(f.app.world().resource::<PipBoy>().phase,Phase::Up);
        assert_eq!(f.app.world().resource::<PipBoy>().clip_time,HOLD_SECONDS);
        assert!(f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
        actual_pose(&mut f,HOLD_SECONDS);
    }
}

#[test]
fn raw_outside_display_end_settles_real_clip_then_native_inventory_closes() {
    let mut f=coupled_scene();
    let screen=f.app.world().resource::<crate::touch::pipboy_model::Geometry>().screen.unwrap();
    let p=Vec2::new(screen.max.x+36.,screen.center().y);
    raw(&mut f,812,TouchPhase::Started,p);
    raw(&mut f,812,TouchPhase::Moved,p+Vec2::new(0.,96.));
    let before=f.app.world().resource::<PipBoy>().clip_time;
    assert!(before>0. && before<HOLD_SECONDS*0.5);
    actual_pose(&mut f,before);
    assert!(f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
    raw(&mut f,812,TouchPhase::Ended,p+Vec2::new(0.,96.));
    for _ in 0..24 { f.app.update(); if !f.app.world().resource::<ArmGesture>().captures()
        && *f.app.world().resource::<State<PlayState>>().get()==PlayState::Playing {break;} }
    assert!(!f.app.world().resource::<ArmGesture>().captures());
    assert_eq!(*f.app.world().resource::<State<PlayState>>().get(),PlayState::Playing);
    assert!(f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
    assert_eq!(f.app.world().resource::<PipBoy>().phase,Phase::Down);
    assert_eq!(f.app.world().resource::<PipBoy>().clip_time,0.);
    assert!(f.app.world().resource::<nvf_play::pad::PlatformInputGate>().allows_actions());
    assert!(f.app.world().resource::<LastAppliedClip>().1,
        "the actual body pass must consume the settled zero-time clip and apply its bind transform before native close");
}

#[test]
fn raw_owned_arm_clip_keeps_exact_pose_with_main_animation_smoothing_on() {
    for cancel in [true, false] {
        let mut f = coupled_scene_with_inertia(true);
        let screen = f.app.world().resource::<crate::touch::pipboy_model::Geometry>().screen.unwrap();
        let p = Vec2::new(screen.max.x + 36., screen.center().y);
        raw(&mut f, 813, TouchPhase::Started, p);
        assert!(f.app.world().resource::<ArmGesture>().captures());
        raw(&mut f, 813, TouchPhase::Moved, p + Vec2::new(40., 60.));
        let lower = f.app.world().resource::<PipBoy>().clip_time;
        assert!(lower > 0. && lower < HOLD_SECONDS);
        assert!(f.app.world().resource::<PipBoy>().manual_pose());
        actual_pose(&mut f, lower);
        raw(&mut f, 813, TouchPhase::Moved, p + Vec2::new(40., 28.));
        let reversed = f.app.world().resource::<PipBoy>().clip_time;
        assert!(reversed > lower && reversed < HOLD_SECONDS);
        actual_pose(&mut f, reversed);
        let rig = f.app.world().resource::<PlayerBody>().body.as_ref().unwrap().first.as_ref().unwrap();
        assert!(!rig.animator.layers.iter().find(|l| l.tag == TAG_PIPBOY).unwrap().smooth,
            "finger ownership retains its original blend law as well as exact sample time");
        if cancel {
            raw(&mut f, 813, TouchPhase::Canceled, p + Vec2::new(40., 28.));
            for _ in 0..24 { if !f.app.world().resource::<ArmGesture>().captures() { break; } f.app.update(); }
            assert!(!f.app.world().resource::<ArmGesture>().captures());
            assert!(!f.app.world().resource::<PipBoy>().manual_pose());
            assert_eq!(f.app.world().resource::<PipBoy>().phase, Phase::Up);
            actual_pose(&mut f, HOLD_SECONDS);
        } else {
            raw(&mut f, 813, TouchPhase::Moved, p + Vec2::new(40., 96.));
            let close = f.app.world().resource::<PipBoy>().clip_time;
            assert!(close < HOLD_SECONDS * 0.5);
            actual_pose(&mut f, close);
            raw(&mut f, 813, TouchPhase::Ended, p + Vec2::new(40., 96.));
            for _ in 0..24 { f.app.update(); if !f.app.world().resource::<ArmGesture>().captures()
                && *f.app.world().resource::<State<PlayState>>().get() == PlayState::Playing { break; } }
            assert!(!f.app.world().resource::<ArmGesture>().captures());
            assert!(!f.app.world().resource::<PipBoy>().manual_pose());
            assert_eq!(f.app.world().resource::<PipBoy>().phase, Phase::Down);
            assert_eq!(f.app.world().resource::<PipBoy>().clip_time, 0.);
            assert!(f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
            assert!(f.app.world().resource::<LastAppliedClip>().1);
        }
        assert!(f.app.world().resource::<nvf_play::inertia::MovementInertia>().smooth_hands(),
            "the test never disables Main smoothing globally");
    }
}

#[test]
fn ordinary_native_raise_keeps_main_inertial_sampling_without_manual_ownership() {
    let mut f = coupled_scene_with_inertia(true);
    f.app.world_mut().write_message(nvf_play::menu::MenuInput(nvf_play::menu::model::Command::Close));
    for _ in 0..40 {
        f.app.update();
        if f.app.world().resource::<PipBoy>().phase == Phase::Down
            && *f.app.world().resource::<State<PlayState>>().get() == PlayState::Playing { break; }
    }
    assert_eq!(f.app.world().resource::<PipBoy>().phase, Phase::Down);
    assert!(f.app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
    f.app.world_mut().write_message(nvf_play::menu::OpenInventory);
    let mut observed = false;
    for _ in 0..24 {
        let pip = f.app.world().resource::<PipBoy>();
        let clock = pip.clip_time;
        let sample = pip.phase == Phase::Raising && clock > 0. && clock < HOLD_SECONDS && !pip.manual_pose();
        let expected = pipboy_clip_time(clock);
        f.app.update();
        if sample {
            let seen = f.app.world().resource::<LastAppliedClip>().0.expect("actual body observer sampled the ordinary raise");
            assert!((seen.0 - expected).abs() < 0.0001);
            assert!((expected - clock).abs() > 0.0001,"interior automatic clock distinguishes the Main warp from raw time");
            assert!(!f.app.world().resource::<PipBoy>().manual_pose());
            assert!(!f.app.world().resource::<ArmGesture>().captures());
            observed = true;
            break;
        }
    }
    assert!(observed,"real native close/open must reach an interior ordinary raise sample");
    assert!(f.app.world().resource::<nvf_play::inertia::MovementInertia>().smooth_hands());
}
