// Included inside the existing sole-reader parent fixture module.
// Empty authored lists test ownership/closing; item/cost/recipe outcomes live in nvf-play.
use bevy::ecs::system::RunSystemOnce;
#[derive(Resource, Default)]
struct ServiceSeen { vendor: usize, workbench: usize }
fn observe_services(
    mut vendor: MessageReader<nvf_play::repair::RepairServicesInput>,
    mut recipe: MessageReader<nvf_play::crafting::RecipeInput>,
    mut seen: ResMut<ServiceSeen>,
) {
    seen.vendor += vendor.read().filter(|r| matches!(r, nvf_play::repair::RepairServicesInput::CommitNormal(_))).count();
    seen.workbench += recipe.read().filter(|r| matches!(r, nvf_play::crafting::RecipeInput::CommitNormal(_))).count();
}
fn service_open(app: &App, vendor: bool) -> bool {
    if vendor { app.world().resource::<nvf_play::repair::RepairServices>().is_open() }
    else { app.world().resource::<nvf_play::crafting::RecipeMenu>().is_open() }
}
fn service_envelopes(app: &App, vendor: bool) -> usize {
    let seen = app.world().resource::<ServiceSeen>();
    if vendor { seen.vendor } else { seen.workbench }
}
fn service_caps(app: &App) -> i32 {
    app.world().resource::<nvf_play::sim::SimState>().world.peek(fnv_sim::FormId::PLAYER_REF)
        .unwrap().inventory.as_ref().unwrap().count(fnv_sim::FormId(0xf))
}
fn service_scene(vendor: bool) -> (App, Entity, Entity) {
    let (mut app, window, camera) = scene_with_sheet(false);
    app.world_mut().resource_mut::<menu::ActiveMenu>().0 = None;
    app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Playing);
    app.world_mut().run_schedule(bevy::state::state::StateTransition);
    app.insert_resource(empty_play_world())
        .insert_resource(nvf_play::script::fixtures::over_plugins(&[]))
        .insert_resource(nvf_play::settings::SettingsFile(None))
        .init_resource::<ServiceSeen>()
        .add_plugins((nvf_play::repair::RepairPlugin, nvf_play::crafting::CraftingPlugin))
        .add_systems(Update, observe_services.after(nvf_play::menu::normal_services::Dispatch));
    let setting = app.world().resource::<nvf_play::settings::Settings>()
        .index(nvf_play::maintenance_touch::SETTING_KEY).unwrap();
    if !(app.world().resource::<nvf_play::settings::Settings>().list()[setting].get)(app.world()) {
        assert_eq!(nvf_play::settings::flip(app.world_mut(), setting), Some(true));
    }
    {
        let mut script = app.world_mut().resource_mut::<nvf_play::script::ScriptState>();
        let game = &mut script.rt.as_mut().unwrap().game;
        // Public script requests enter the original scripts_open systems; no Open/View seed.
        if vendor {
            game.open_menu(fnv_game::repair::REPAIR_SERVICES_MENU);
            game.repair_requests.push(fnv_game::repair::RepairRequest { vendor: fnv_sim::FormId::PLAYER_REF });
        } else {
            game.open_menu(fnv_game::crafting::RECIPE_MENU);
            game.recipe_requests.push(fnv_game::crafting::RecipeRequest { station: None, category: fnv_sim::FormId(0xfa90) });
        }
    }
    for _ in 0..8 { app.update(); }
    assert!(service_open(&app, vendor), "the actual script request opened its ordinary menu");
    assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(), nvf_play::PlayState::Menu);
    assert!(app.world().resource::<nvf_play::menu::normal_services::NormalServices>().visible());
    assert!(!app.world_mut().run_system_once(|b: nvf_play::maintenance_touch::Bridge| b.captures_touch()).unwrap(), "vendor/workbench never activate the alternate sheet");
    assert!(!app.world().resource::<normal_touch::NormalBarter>().visible());
    assert_eq!(service_envelopes(&app, vendor), 0);
    assert_eq!(service_caps(&app), 1000);
    (app, window, camera)
}
fn service_exit_point(app: &mut App, vendor: bool) -> Vec2 {
    let prefix = if vendor { "Done" } else { "Exit" };
    let world = app.world_mut();
    let mut texts = world.query::<(Entity, &Text)>();
    let mut entity = texts.iter(world).find_map(|(e,t)| t.0.starts_with(prefix).then_some(e))
        .expect("ordinary painter emitted the real closing hint");
    for _ in 0..16 {
        if world.get::<ui_pointer::Button>(entity).is_some() {
            let node = world.get::<ComputedNode>(entity).unwrap();
            assert!(node.size().min_element() > 0., "real closing button was laid out");
            let at = world.get::<bevy::ui::UiGlobalTransform>(entity).unwrap().translation;
            let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
            return at / windows.single(world).unwrap().scale_factor();
        }
        entity = world.get::<ChildOf>(entity).expect("closing hint has its actual button ancestor").parent();
    }
    panic!("ordinary service closing-button ancestry exceeded bound")
}
fn service_has_no_underlay_effect(app: &App) {
    assert_eq!(app.world().resource::<Seen>().underlay_presses, 0);
    assert_eq!(app.world().resource::<Seen>().envelopes, 0, "service input must not become barter input");
    let generic = app.world().resource::<DefaultMenuTouchSeen>();
    assert_eq!((generic.presses, generic.releases, generic.scrolls), (0,0,0));
    assert!(app.world().resource::<TouchState>().contacts.is_empty());
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
    assert_eq!(service_caps(app), 1000);
}
#[test]
fn sole_raw_services_router_uses_actual_script_open_painter_and_original_close_consumer() {
    for vendor in [true, false] {
        let (mut app, window, camera) = service_scene(vendor);
        let point = service_exit_point(&mut app, vendor);
        let button = underlay(&mut app, camera, point);
        touch(&mut app, window, 7100, TouchPhase::Started, point.x, point.y); app.update();
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(7100));
        assert_ne!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed);
        assert_eq!(service_envelopes(&app, vendor), 0, "Started cannot close the ordinary menu");
        touch(&mut app, window, 7100, TouchPhase::Ended, point.x, point.y); app.update();
        for _ in 0..4 { app.update(); }
        assert_eq!(service_envelopes(&app, vendor), 1, "one real typed envelope reaches the original keys consumer");
        assert!(!service_open(&app, vendor));
        assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(), nvf_play::PlayState::Playing);
        assert!(!app.world().resource::<nvf_play::menu::normal_services::NormalServices>().visible());
        service_has_no_underlay_effect(&app);
    }
}
#[test]
fn sole_raw_services_router_pause_retires_held_close_until_fresh_contact() {
    for vendor in [true, false] {
        let (mut app, window, camera) = service_scene(vendor);
        let point = service_exit_point(&mut app, vendor); underlay(&mut app, camera, point);
        touch(&mut app, window, 7101, TouchPhase::Started, point.x, point.y); app.update();
        app.insert_resource(nvf_play::pause::Paused(true));
        touch(&mut app, window, 7101, TouchPhase::Ended, point.x, point.y); app.update();
        assert!(service_open(&app, vendor)); assert_eq!(service_envelopes(&app, vendor), 0);
        app.world_mut().resource_mut::<nvf_play::pause::Paused>().0 = false;
        for _ in 0..6 { app.update(); }
        assert!(service_open(&app, vendor), "unpausing cannot replay the held release");
        let point = service_exit_point(&mut app, vendor); raw_tap(&mut app, window, 7102, point);
        for _ in 0..3 { app.update(); }
        assert!(!service_open(&app, vendor)); assert_eq!(service_envelopes(&app, vendor), 1);
        service_has_no_underlay_effect(&app);
    }
}
#[test]
fn sole_raw_services_router_registered_off_on_cancels_same_frame_held_release() {
    for vendor in [true, false] {
        let (mut app, window, camera) = service_scene(vendor);
        let point = service_exit_point(&mut app, vendor); underlay(&mut app, camera, point);
        touch(&mut app, window, 7103, TouchPhase::Started, point.x, point.y); app.update();
        let index = app.world().resource::<nvf_play::settings::Settings>()
            .index(nvf_play::maintenance_touch::SETTING_KEY).unwrap();
        assert_eq!(nvf_play::settings::flip(app.world_mut(), index), Some(false));
        assert_eq!(nvf_play::settings::flip(app.world_mut(), index), Some(true));
        touch(&mut app, window, 7103, TouchPhase::Ended, point.x, point.y); app.update();
        assert!(service_open(&app, vendor)); assert_eq!(service_envelopes(&app, vendor), 0);
        for _ in 0..6 { app.update(); }
        let point = service_exit_point(&mut app, vendor); raw_tap(&mut app, window, 7104, point);
        for _ in 0..3 { app.update(); }
        assert!(!service_open(&app, vendor)); assert_eq!(service_envelopes(&app, vendor), 1);
        service_has_no_underlay_effect(&app);
    }
}
#[test]
fn sole_raw_services_router_consumed_controller_return_needs_no_extra_restore_tap() {
    for vendor in [true, false] {
        let (mut app, window, camera) = service_scene(vendor);
        let point = service_exit_point(&mut app, vendor);
        let button = underlay(&mut app, camera, point);
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = true;
        nvf_play::bindings::set_device(app.world_mut(), nvf_play::bindings::InputDevice::Controller);
        for _ in 0..3 { app.update(); }
        assert!(!app.world().resource::<nvf_play::menu::normal_services::NormalServices>().visible());
        touch(&mut app, window, 7105, TouchPhase::Started, point.x, point.y); app.update();
        assert_eq!(app.world().resource::<Bindings>().device, nvf_play::bindings::InputDevice::KeyboardMouse);
        assert!(!app.world().resource::<nvf_play::pad::TouchDeviceSwitch>().allows_actions());
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(7105));
        assert_ne!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed);
        assert!(service_open(&app, vendor)); assert_eq!(service_envelopes(&app, vendor), 0);
        touch(&mut app, window, 7105, TouchPhase::Ended, point.x, point.y); app.update();
        for _ in 0..4 { app.update(); }
        assert!(app.world().resource::<nvf_play::menu::normal_services::NormalServices>().visible());
        let point = service_exit_point(&mut app, vendor); raw_tap(&mut app, window, 7106, point);
        for _ in 0..3 { app.update(); }
        assert!(!service_open(&app, vendor)); assert_eq!(service_envelopes(&app, vendor), 1);
        service_has_no_underlay_effect(&app);
    }
}
