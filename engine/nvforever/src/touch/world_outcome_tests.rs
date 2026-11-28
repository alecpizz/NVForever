//! Sole raw TouchInput reader through actual projection and native door dispatch.
use super::*;
use bevy::{
    camera::primitives::Aabb,
    camera::{CameraProjection, RenderTarget, RenderTargetInfo},
    ecs::message::Messages,
};
use fnv_sim::forms::{ActorBase, BaseData, BaseForm, Door};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig};

pub(super) fn scene(mode: &str) -> (App, Entity, FormId, Vec2) {
    let (mut app, window) = super::tests::scene();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(16),
    ));
    app.insert_resource(nvf_play::settings::SettingsFile(None));
    let index = app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .index("touch-world-activation-v1")
        .unwrap();
    assert!(nvf_play::settings::pick(app.world_mut(), index, mode));
    let mut forms = FormStore::default();
    for (id, data) in [
        (
            FormId::PLAYER_BASE,
            BaseData::Actor(ActorBase {
                entries: vec![],
                attributes: None,
            }),
        ),
        (
            FormId(0xF200),
            BaseData::Door(Door {
                automatic: false,
                minimal_use: false,
            }),
        ),
    ] {
        forms.bases.insert(
            id,
            BaseForm {
                id,
                sig: *b"DOOR",
                edid: None,
                name: Some("Owned fixture".into()),
                script: None,
                prompt: None,
                voice_filter: false,
                data,
            },
        );
    }
    // World::place uses the signature index, as FormStore::build populates it.
    // Keep both indices of these authored fixture forms in agreement.
    for (&id, base) in &forms.bases {
        forms.sigs.insert(id, base.sig);
    }
    let forms = Box::leak(Box::new(forms));
    let refs = Box::leak(Box::new(RefTable::default()));
    let mut sim = nvf_play::sim::SimState {
        forms,
        refs,
        world: fnv_sim::World::new(forms, refs, SimConfig::default()),
    };
    let point = {
        let config = &app.world().resource::<Layout>().config;
        (560..800)
            .step_by(20)
            .flat_map(|x| {
                (160..440)
                    .step_by(20)
                    .map(move |y| Vec2::new(x as f32, y as f32))
            })
            .find(|p| {
                config
                    .hit_filtered(*p, Vec2::splat(1000.), true, false, |_| true)
                    .is_none()
                    && !layout::utility_rect(false, Vec2::splat(1000.)).contains(*p)
                    && !layout::utility_rect(true, Vec2::splat(1000.)).contains(*p)
            })
            .unwrap()
    };
    let mut camera = Camera::default();
    camera.computed.clip_from_view = PerspectiveProjection {
        aspect_ratio: 1.,
        ..default()
    }
    .get_clip_from_view();
    camera.computed.target_info = Some(RenderTargetInfo {
        physical_size: UVec2::splat(1000),
        scale_factor: 1.,
    });
    let transform = GlobalTransform::from(
        Transform::from_translation(nvf_play::space::to_bevy([0., 0., 100.]))
            .looking_to(nvf_play::space::to_bevy([1., 0., 0.]), Vec3::Y),
    );
    let ray = camera.viewport_to_world(&transform, point).unwrap();
    let at = nvf_play::space::to_game(transform.translation() + ray.direction.as_vec3() * 90.);
    assert_eq!(sim.forms.sig(FormId(0xF200)), Some(*b"DOOR"));
    let reference = sim
        .world
        .place(FormId(0xF200), None, at, [0.; 3], 1)
        .unwrap();
    assert_eq!(sim.base_of(reference.0), Some(FormId(0xF200)));
    assert!(sim.present(reference.0));
    assert!(!sim.world.peek(reference).unwrap().open);
    app.world_mut().spawn((
        camera,
        transform,
        RenderTarget::Window(bevy::window::WindowRef::Primary),
        nvf_play::PlayerCamera,
    ));
    app.world_mut().spawn((
        nvf_play::RefVisual(reference.0),
        Aabb {
            center: Vec3::ZERO.into(),
            half_extents: Vec3::splat(2.).into(),
        },
        GlobalTransform::from_translation(nvf_play::space::to_bevy(at)),
        InheritedVisibility::VISIBLE,
        Visibility::Inherited,
    ));
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).unwrap()));
    app.insert_resource(sim)
        .insert_resource(nvf_play::PlayWorld(nvf_play::PlayData {
            lo,
            schema: Box::leak(Box::default()),
            index: Box::leak(Box::default()),
            world: 0,
            interior: None,
            eye: [0.; 3],
            heading_deg: 90.,
            pitch_deg: 0.,
            auto_walk: 0.,
            start_at_cell_spot: false,
            packages: false,
        }))
        .insert_resource(nvf_play::player::Player {
            heading: std::f32::consts::FRAC_PI_2,
            pitch: 0.,
            eye_height: 100.,
            feet: [0.; 3],
            body_eye: None,
            body_view: None,
            placed: None,
        })
        .init_resource::<nvf_play::furniture::PlayerSeat>()
        .add_message::<nvf_play::loading::TravelRequest>()
        .add_message::<nvf_play::OpenContainer>()
        .add_message::<nvf_play::RefChanged>()
        .add_message::<nvf_play::furniture::UseFurniture>()
        .add_message::<nvf_play::world_sounds::Activated>()
        .add_message::<nvf_play::terminal::OpenTerminal>()
        .add_message::<nvf_play::lockpick::OpenLockpick>()
        .add_message::<nvf_play::pickpocket::OpenPickpocket>()
        .add_message::<nvf_play::pickpocket::Pickpocketed>()
        .add_message::<nvf_play::ui_sounds::UiSound>()
        // Main's ActivatePlugin runs Quick Loot's `follow` before `activate`; the
        // production QuickLootPlugin supplies its state and the crime plugin its message.
        .init_resource::<nvf_play::quick_loot::QuickLoot>()
        .add_message::<nvf_play::crime::ItemsTaken>()
        .add_plugins(nvf_play::activate::ActivatePlugin);
    app.update();
    (app, window, reference, point)
}
fn edge(app: &mut App, w: Entity, id: u64, phase: TouchPhase, p: Vec2) {
    super::tests::touch(app, w, id, phase, p.x, p.y);
}
fn opened(app: &App, id: FormId) -> bool {
    app.world()
        .resource::<nvf_play::sim::SimState>()
        .world
        .peek(id)
        .unwrap()
        .open
}
fn activations(app: &mut App) -> usize {
    app.world_mut()
        .resource_mut::<Messages<nvf_play::world_sounds::Activated>>()
        .drain()
        .count()
}

#[test]
fn raw_world_tap_reaches_native_door_once_and_never_attacks() {
    let (mut app, w, id, p) = scene("Tap");
    edge(&mut app, w, 90, TouchPhase::Started, p);
    app.update();
    assert!(!opened(&app, id));
    assert!(app
        .world()
        .resource::<nvf_play::activate::Target>()
        .0
        .is_none());
    edge(&mut app, w, 90, TouchPhase::Ended, p);
    app.update();
    assert!(opened(&app, id));
    assert_eq!(activations(&mut app), 1);
    assert!(!app
        .world()
        .resource::<ButtonInput<MouseButton>>()
        .pressed(MouseButton::Left));
    for _ in 0..3 {
        app.update();
    }
    assert!(opened(&app, id));
    assert_eq!(activations(&mut app), 0);
}

#[test]
fn raw_double_tap_drag_and_same_batch_ui_edge_keep_native_confirmation_separate() {
    let (mut app, w, id, p) = scene("Double tap");
    edge(&mut app, w, 91, TouchPhase::Started, p);
    app.update();
    edge(&mut app, w, 91, TouchPhase::Ended, p);
    app.update();
    assert!(!opened(&app, id));
    edge(&mut app, w, 92, TouchPhase::Started, p);
    app.update();
    edge(&mut app, w, 92, TouchPhase::Ended, p);
    app.update();
    assert!(opened(&app, id));
    assert_eq!(activations(&mut app), 1);
    let i = app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .index("touch-world-activation-v1")
        .unwrap();
    assert!(nvf_play::settings::pick(app.world_mut(), i, "Tap"));
    edge(&mut app, w, 93, TouchPhase::Started, p);
    app.update();
    edge(&mut app, w, 93, TouchPhase::Moved, p + Vec2::new(20., 0.));
    edge(&mut app, w, 93, TouchPhase::Ended, p);
    app.update();
    assert!(opened(&app, id));
    assert_eq!(activations(&mut app), 0);
    edge(&mut app, w, 94, TouchPhase::Started, p);
    app.update();
    edge(&mut app, w, 94, TouchPhase::Ended, p);
    let pause = app.world().resource::<Layout>().config.center(Target::Button(0), Vec2::splat(1000.));
    edge(&mut app, w, 95, TouchPhase::Started, pause);
    app.update();
    assert!(
        opened(&app, id),
        "the later UI edge must cancel an unsent world confirmation"
    );
    assert_eq!(activations(&mut app), 0);
}


#[test]
fn actual_raw_world_hold_platform_block_drops_old_release_and_admits_fresh_native_activation(){
    let (mut app,w,id,p)=scene("Tap");
    #[derive(Resource,Default)]struct Heard(usize);
    fn hear(mut events:MessageReader<nvf_play::world_sounds::Activated>,mut heard:ResMut<Heard>){
        heard.0+=events.read().count();
    }
    app.init_resource::<Heard>().add_systems(Update,hear.after(nvf_play::PlaySet::Activation));
    let choice=app.world().resource::<nvf_play::settings::Settings>().index("touch-world-activation-v1").unwrap();
    assert!(nvf_play::settings::pick(app.world_mut(),choice,"Hold"));
    app.init_resource::<nvf_play::pad::PlatformInputGate>().add_plugins(nvf_play::pad::InputSwitchPlugin);
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput{window:w,key_code:KeyCode::F12,
        logical_key:bevy::input::keyboard::Key::F12,state:ButtonState::Pressed,text:None,repeat:false});
    app.update();
    edge(&mut app,w,900,TouchPhase::Started,p);app.update();
    for _ in 0..16{app.update();}
    assert!(app.world().resource::<nvf_play::activate::touch::TouchActivation>().progress()>0.0);
    assert!(!opened(&app,id));
    app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);app.update();
    assert!(app.world().resource::<nvf_play::activate::touch::TouchActivation>().point().is_none());
    assert!(app.world().resource::<TouchState>().contacts.is_empty());
    assert!(!app.world().resource::<nvf_play::control::HeldSneakInput>().active);
    edge(&mut app,w,900,TouchPhase::Ended,p);app.update();
    app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(false);
    for _ in 0..42{app.update();}
    assert!(!opened(&app,id),"an old release or elapsed hold cannot activate after platform ownership ends");
    assert_eq!(app.world().resource::<Heard>().0,0);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    edge(&mut app,w,901,TouchPhase::Started,p);app.update();
    assert_eq!(app.world().resource::<nvf_play::activate::touch::TouchActivation>().point().map(|point|point.id),Some(901),
        "the parent must also retire its wrapper world owner, not merely the core probe");
    for _ in 0..42{app.update();}
    assert!(opened(&app,id));assert_eq!(app.world().resource::<Heard>().0,1);
    edge(&mut app,w,901,TouchPhase::Canceled,p);app.update();
    for _ in 0..5{app.update();}
    assert!(opened(&app,id));assert_eq!(app.world().resource::<Heard>().0,1);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
}
