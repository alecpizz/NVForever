//! Authored inventory records, real raw touch admission, MenuInput and apply_input.
//! The alternate presentation is exercised with a genuinely built native menu and
//! with the ordinary hand-built fallback. Shared wrapper arbitration is separate.
use super::*;
use bevy::{camera::{ComputedCameraValues, RenderTargetInfo, Viewport}, input::{InputPlugin, InputSystems},
    input::touch::Touches, state::app::StatesPlugin, window::WindowRef};
use fnv_esm::{raw::{Subrecord, write_subrecords}, LoadOrder, Plugin, Schema};
use fnv_sim::{FormStore, RefTable, SimConfig};

const CAPS: FormId = FormId(0xf);
const GOODS: FormId = FormId(0x200);
const BOX: FormId = FormId(0x300);

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord { Subrecord { sig: *sig, data, extended: false } }
fn z(s: &str) -> Vec<u8> { let mut data = s.as_bytes().to_vec(); data.push(0); data }
fn record(sig: &[u8; 4], id: u32, subs: Vec<Subrecord>) -> Vec<u8> {
    let data = write_subrecords(&subs);
    [sig.as_slice(), &(data.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &id.to_le_bytes(),
        &[0; 4], &15u16.to_le_bytes(), &[0; 2], &data].concat()
}
fn top(sig: &[u8; 4], body: Vec<u8>) -> Vec<u8> {
    [b"GRUP".as_slice(), &((24 + body.len()) as u32).to_le_bytes(), sig,
        &0i32.to_le_bytes(), &[0; 8], &body].concat()
}
fn simulation() -> (crate::sim::SimState, FormId) {
    let mut misc = Vec::new();
    for (id, name, value) in std::iter::once((CAPS, "Coins".to_string(), 1i32))
        .chain((0..10).map(|i| (FormId(GOODS.0 + i), format!("Supply {i:02}"), 10))) {
        misc.extend(record(b"MISC", id.0, vec![sub(b"EDID", z(&format!("Fixture{}", id.0))), sub(b"FULL", z(&name)),
            sub(b"DATA", [&value.to_le_bytes()[..], &1f32.to_le_bytes()].concat())]));
    }
    let container = record(b"CONT", BOX.0, vec![sub(b"EDID", z("TouchStock")), sub(b"FULL", z("Merchant")),
        sub(b"DATA", [&[0u8][..], &0f32.to_le_bytes()].concat())]);
    let player = record(b"NPC_", 7, vec![sub(b"EDID", z("Player")),
        sub(b"DATA", [&100i32.to_le_bytes()[..], &[5; 7]].concat())]);
    let bytes = [record(b"TES4", 0, vec![sub(b"HEDR", [&1.34f32.to_le_bytes()[..], &0i32.to_le_bytes(), &0x800u32.to_le_bytes()].concat())]),
        top(b"MISC", misc), top(b"CONT", container), top(b"NPC_", player)].concat();
    let plugins = [Plugin::parse(&bytes, "AuthoredTouch.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let forms = Box::leak(Box::new(FormStore::build(&order, &schema)));
    assert!(forms.issues.is_empty(), "{:?}", forms.issues.to_json());
    let refs = Box::leak(Box::new(RefTable::build(&order, &schema)));
    let mut world = fnv_sim::World::new(forms, refs, SimConfig::default());
    let stock = world.place(BOX, None, [0.0; 3], [0.0; 3], 1).unwrap();
    for holder in [PLAYER, stock] {
        world.add_item(holder, CAPS, 1000).unwrap();
        for i in 0..10 { world.add_item(holder, FormId(GOODS.0 + i), 8).unwrap(); }
    }
    (crate::sim::SimState { forms, refs, world }, stock)
}

#[derive(Resource, Clone, Copy)]
struct Fixture(Frame);
#[derive(Resource, Default)]
struct LateChange(Option<u8>);
#[derive(Resource, Default)]
struct LateConsumer(Option<u8>);
#[derive(Resource, Default)]
struct LastEnvelope(Option<u64>);
#[derive(Resource, Default)]
struct RepeatEnvelope { next: bool, queued: usize }
#[derive(Resource, Default)]
struct BeforeConsumerCount(usize);
fn mutate_after_raw(world: &mut bevy::prelude::World) {
    let Some(change) = world.resource_mut::<LateChange>().0.take() else { return };
    apply_late_change(world, change);
}
fn mutate_before_consumer(world: &mut bevy::prelude::World) {
    let Some(change) = world.resource_mut::<LateConsumer>().0.take() else { return };
    apply_late_change(world, change);
}
fn remember_envelope(mut input: MessageReader<MenuInput>, mut last: ResMut<LastEnvelope>,
    mut repeat: ResMut<RepeatEnvelope>, mut commands: Commands) {
    for event in input.read() {
        if let Command::CommitBarter(token) = event.0 {
            last.0 = Some(token);
            if std::mem::take(&mut repeat.next) {
                repeat.queued += 1;
                commands.queue(move |world: &mut bevy::prelude::World| {
                    world.write_message(MenuInput(Command::CommitBarter(token)));
                });
            }
        }
    }
}
fn count_before_consumer(input: Res<Messages<MenuInput>>, last: Res<LastEnvelope>, mut seen: ResMut<BeforeConsumerCount>) {
    seen.0 = last.0.map_or(0, |token| input.iter_current_update_messages()
        .filter(|event| event.0 == Command::CommitBarter(token)).count());
}
fn apply_late_change(world: &mut bevy::prelude::World, change: u8) {
    let f = world.resource::<Fixture>().0;
    match change {
        0 => world.resource_mut::<crate::touch_safe_area::SafeInsets>().left = 8.0,
        1 => world.resource_mut::<UiScale>().0 = 2.0,
        2 => {
            let mut w = world.get_mut::<Window>(f.window).unwrap();
            w.resolution.set_scale_factor_override(Some(2.0));
            w.resolution.set_physical_resolution(1280, 720);
        }
        3 => world.resource_mut::<crate::menu::FreshMenu>().0 = true,
        4 => world.resource_mut::<crate::bindings::KeyboardCapture>().0 = true,
        5 => world.resource_mut::<crate::free_cam::FreeCam>().active = true,
        6 => world.resource_mut::<crate::pause::Paused>().0 = true,
        7 => world.get_mut::<Camera>(f.camera).unwrap().is_active = false,
        8 => world.resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true),
        9 => world.resource_mut::<crate::bindings::Bindings>().device = crate::bindings::InputDevice::Controller,
        10 => world.resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.open_menu(fnv_game::game::menus::MESSAGE),
        11 => {
            let mut camera = world.get_mut::<Camera>(f.camera).unwrap();
            camera.viewport = Some(Viewport { physical_size: UVec2::new(640,360), ..default() });
            camera.computed.target_info.as_mut().unwrap().physical_size = UVec2::new(1280,720);
        }
        12 => world.resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.loading = true,
        13 => world.resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.vats.mode = fnv_game::vats::MODE_TARGET_SELECT,
        _ => unreachable!(),
    }
}
fn raw_owner(frame: Res<Fixture>, touches: Res<Touches>, mut events: MessageReader<TouchInput>, mut bridge: Bridge) {
    let neutral = touches.iter().next().is_none() && !touches.any_just_released() && !touches.any_just_canceled();
    bridge.prepare(Frame { neutral, ..frame.0 });
    for edge in events.read() { bridge.event(edge); }
}
fn scene(native: bool, phase: crate::PlayState) -> (App, FormId) {
    use crate::tiles::{barter_menu, container_menu, input, vanilla::TileMenus};
    let (mut sim, stock) = simulation();
    let m = MenuModel::open_barter(&mut sim.world, stock, stock, CAPS, 100.0, 0).unwrap();
    let mut script = crate::script::ScriptState::over_no_plugins();
    crate::script::engine_menu_opened(Some(&mut script), m.mode);
    let mut tiles = barter_menu::BarterTiles::default();
    if native {
        let mut ui = barter_menu::tests::fixture(|s| s);
        let snap = barter_menu::Snapshot::of(&m, None, "Yours".into(), "Coins".into(), ["A".into(), "X".into()]);
        barter_menu::tests::frame(&mut ui, &snap, &mut tiles);
        assert!(barter_menu::built(&ui).yes());
        let game = &mut script.rt.as_mut().unwrap().game;
        game.ui_tree = ui;
        game.ui_files = Some(Box::new(fnv_ui::MemSource::default()));
        game.menu = Some(crate::script::MENU_BARTER);
        assert!(barter_menu::tiled(true, game, true));
    }
    let mut app = App::new();
    app.add_plugins((StatesPlugin, InputPlugin)).insert_state(phase)
        .insert_resource(sim).insert_resource(script).insert_resource(ActiveMenu(Some(m)))
        .insert_resource(TileMenus { barter: native, ..default() }).insert_resource(tiles)
        .insert_resource(Options { enabled: true })
        .init_resource::<LateChange>().init_resource::<LateConsumer>().init_resource::<LastEnvelope>().init_resource::<RepeatEnvelope>()
        .init_resource::<BeforeConsumerCount>()
        .init_resource::<UiScale>().init_resource::<crate::touch_safe_area::SafeInsets>()
        .init_resource::<crate::bindings::KeyboardCapture>().init_resource::<crate::free_cam::FreeCam>().init_resource::<crate::pause::Paused>()
        .init_resource::<crate::pad::PlatformInputGate>().init_resource::<crate::bindings::Bindings>()
        .init_resource::<crate::menu::PromptKeys>().init_resource::<crate::menu::FreshMenu>().init_resource::<crate::menu::WheelHold>()
        .init_resource::<Time>().init_resource::<crate::pause::PlayClock>()
        .init_resource::<crate::pad::PadGlyphs>().init_resource::<Assets<Image>>()
        .init_resource::<input::MessageClick>().init_resource::<input::WaitSliderCheck>()
        .init_resource::<container_menu::ContainerTiles>().init_resource::<container_menu::TileWheelTitle>()
        .add_message::<crate::RefChanged>().add_message::<crate::ItemUsed>()
        .add_message::<crate::ui_sounds::UiSound>().add_message::<crate::crime::ItemsTaken>()
        .add_message::<crate::pickpocket::Pickpocketed>().add_message::<crate::wait::WaitInput>()
        .add_message::<crate::crafting::RecipeInput>().add_message::<crate::repair::RepairServicesInput>()
        .add_message::<crate::tutorial_menu::TutorialInput>().add_message::<CursorMoved>();
    let window = app.world_mut().spawn((Window { resolution: (640, 360).into(), focused: true, ..default() }, PrimaryWindow)).id();
    let camera = app.world_mut().spawn((Camera2d, Camera { computed: ComputedCameraValues {
        target_info: Some(RenderTargetInfo { physical_size: UVec2::new(640, 360), scale_factor: 1.0 }), ..default()
    }, ..default() }, RenderTarget::Window(WindowRef::Entity(window)))).id();
    app.insert_resource(Fixture(Frame { window, camera, size: Vec2::new(640.0, 360.0), safe: default(),
        ui_scale: 1.0, density: 1.0, admitted: true, fresh: false, neutral: true }));
    install(&mut app);
    app.add_systems(PreUpdate, raw_owner.after(InputSystems))
        .add_systems(Update, mutate_after_raw.before(Dispatch))
        .add_systems(Update, (mutate_before_consumer, remember_envelope, ApplyDeferred, count_before_consumer)
            .chain().after(Dispatch).before(crate::menu::MenuSystems))
        .add_systems(Update, (input::feed, crate::menu::quantity_keys, crate::menu::menu_keys,
            crate::menu::menu_mouse, crate::menu::quantity_mouse, crate::menu::apply_input,
            crate::menu::draw).chain().in_set(crate::menu::MenuSystems));
    app.update();
    let fallback_rows = app.world_mut().query_filtered::<Entity, With<crate::menu::RowButton>>().iter(app.world()).count();
    if native { assert_eq!(fallback_rows, 0, "real draw selected the built native presentation"); }
    else { assert!(fallback_rows > 0, "real draw selected authored hand-built rows"); }
    (app, stock)
}
fn model(app: &App) -> &MenuModel { app.world().resource::<ActiveMenu>().0.as_ref().unwrap() }
fn at(app: &App, target: Target) -> Vec2 {
    let s = app.world().resource::<TouchBarter>();
    view::Panel::of(s.frame.unwrap()).unwrap().hits(model(app), s.first).into_iter()
        .find(|h| h.target == target).unwrap().rect.center()
}
fn row(app: &App, side: Side, item: FormId) -> RowToken {
    let m = model(app);
    RowToken::at(m, side, m.list(side).rows.iter().position(|r| r.item == item).unwrap()).unwrap()
}
fn edge(app: &mut App, id: u64, phase: TouchPhase, position: Vec2) {
    let window = app.world().resource::<Fixture>().0.window;
    app.world_mut().write_message(TouchInput { window, id, phase, position, force: None });
}
fn tap(app: &mut App, target: Target) {
    let point = at(app, target);
    edge(app, 1, TouchPhase::Started, point); app.update();
    edge(app, 1, TouchPhase::Ended, point); app.update();
    app.update();
}
fn count(app: &mut App, holder: FormId, item: FormId) -> i32 {
    app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(holder, item).unwrap()
}
fn keyboard(app: &mut App, down: bool) {
    use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState};
    let window = app.world().resource::<Fixture>().0.window;
    app.world_mut().write_message(KeyboardInput { key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()), state: if down { ButtonState::Pressed } else { ButtonState::Released },
        text: None, repeat: false, window });
    app.update();
}
fn offered(app: &App) -> i32 { model(app).trade.as_ref().unwrap().offered.iter().flat_map(|s| s.values()).sum() }

#[test]
fn actual_touch_barter_buys_and_sells_only_after_amount_then_accept_in_both_presentations() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token));
        assert_eq!(offered(&app), 0, "a row tap only selects, native={native}");
        assert_eq!(model(&app).focus, Side::Container);
        tap(&mut app, Target::One);
        assert_eq!(model(&app).quantity.unwrap().max, 8);
        assert_eq!(offered(&app), 0);
        // This is the real engine Quantity publication (tiles::publish). It is
        // not Game::menu, and may clear one frame after AmountOk applies.
        app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.ui_prompt
            = Some(fnv_game::game::menus::QUANTITY);
        tap(&mut app, Target::Min);
        assert_eq!(model(&app).quantity.unwrap().amount, 1);
        tap(&mut app, Target::Step(1));
        assert_eq!(model(&app).quantity.unwrap().amount, 2);
        tap(&mut app, Target::ConfirmAmount);
        assert_eq!(offered(&app), 2);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        tap(&mut app, Target::Accept);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (10, 6));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (978, 1022));
        assert_eq!(offered(&app), 0);
        let token = row(&app, Side::Player, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::One);
        tap(&mut app, Target::Min); tap(&mut app, Target::ConfirmAmount); tap(&mut app, Target::Accept);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (9, 7));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (987, 1013));
        for _ in 0..4 { app.update(); }
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (9, 7), "no idle duplicate");
    }
}

#[test]
fn actual_touch_barter_both_lists_scroll_and_categories_never_stage_or_transfer() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        for side in [Side::Player, Side::Container] {
            let point = at(&app, Target::Row(row(&app, side, GOODS)));
            edge(&mut app, 2, TouchPhase::Started, point); app.update();
            edge(&mut app, 2, TouchPhase::Moved, point - Vec2::Y * 70.0); app.update();
            edge(&mut app, 2, TouchPhase::Ended, point - Vec2::Y * 70.0); app.update();
            assert!(model(&app).selected[side as usize] > 0);
            assert_eq!(offered(&app), 0); assert!(model(&app).quantity.is_none());
            assert_eq!(count(&mut app, stock, GOODS), 8);
            tap(&mut app, Target::Category(side, 1));
            assert_eq!(model(&app).category[side as usize], Category::Weapons);
            assert!(model(&app).list(side).rows.is_empty());
            tap(&mut app, Target::Category(side, -1));
            assert_eq!(model(&app).category[side as usize], Category::All);
        }
        assert_eq!(offered(&app), 0);
    }
}

#[test]
fn actual_touch_barter_unrelated_and_overlapping_fingers_retain_the_real_owner() {
    let (mut app, _) = scene(true, crate::PlayState::Menu);
    let token = row(&app, Side::Container, GOODS);
    let point = at(&app, Target::Row(token));
    edge(&mut app, 4, TouchPhase::Started, point); app.update();
    for (id, phase) in [(5, TouchPhase::Started), (5, TouchPhase::Canceled), (6, TouchPhase::Moved), (6, TouchPhase::Ended)] {
        edge(&mut app, id, phase, point); app.update();
        assert_eq!(app.world().resource::<TouchBarter>().contact.as_ref().unwrap().id, 4);
        assert_eq!(offered(&app), 0);
    }
    edge(&mut app, 4, TouchPhase::Ended, point); app.update();
    assert_eq!(model(&app).focus, Side::Container);
    assert_eq!(model(&app).selected[0], token.index);
    assert_eq!(offered(&app), 0);
    assert!(app.world().resource::<TouchBarter>().contact.is_none());
    tap(&mut app, Target::One);
    assert!(model(&app).quantity.is_some(), "fresh ordinary gesture still works");
}

#[test]
fn actual_touch_barter_cancel_and_insufficient_caps_do_not_move_items() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        app.world_mut().resource_mut::<crate::sim::SimState>().world.inventory_mut(PLAYER).unwrap().remove(CAPS, 1000);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token));
        tap(&mut app, Target::One); tap(&mut app, Target::CancelAmount);
        assert_eq!(offered(&app), 0);
        tap(&mut app, Target::All); assert_eq!(offered(&app), 8);
        tap(&mut app, Target::Accept);
        assert_eq!(model(&app).status.as_deref(), Some("You don't have enough caps."));
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        assert_eq!(offered(&app), 8, "refusal keeps the existing basket behavior");
        tap(&mut app, Target::Close);
        assert!(matches!(app.world().resource::<NextState<crate::PlayState>>(), NextState::Pending(crate::PlayState::Playing))
            || *app.world().resource::<State<crate::PlayState>>().get() == crate::PlayState::Playing);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
    }
}

#[test]
fn actual_touch_barter_changed_row_quote_and_late_quantity_refuse_held_commit() {
    let (mut app, stock) = scene(false, crate::PlayState::Menu);
    let token = row(&app, Side::Container, GOODS);
    tap(&mut app, Target::Row(token));
    let point = at(&app, Target::All);
    edge(&mut app, 7, TouchPhase::Started, point); app.update();
    app.world_mut().resource_mut::<crate::sim::SimState>().world.inventory_mut(stock).unwrap().remove(GOODS, 8);
    edge(&mut app, 7, TouchPhase::Ended, point); app.update();
    assert_eq!(offered(&app), 0);
    assert_eq!(count(&mut app, PLAYER, GOODS), 8);
    assert!(model(&app).status.as_deref().unwrap().starts_with("Trade changed"));
    let token = row(&app, Side::Player, GOODS);
    tap(&mut app, Target::Row(token)); tap(&mut app, Target::All);
    let point = at(&app, Target::Accept);
    edge(&mut app, 8, TouchPhase::Started, point); app.update();
    app.world_mut().resource_mut::<crate::sim::SimState>().world.inventory_mut(stock).unwrap().remove(CAPS, 999);
    edge(&mut app, 8, TouchPhase::Ended, point); app.update();
    assert_eq!(count(&mut app, PLAYER, GOODS), 8);
    assert_eq!(offered(&app), 8);
    assert!(model(&app).status.as_deref().unwrap().starts_with("Trade changed"));
    let (mut app, _) = scene(true, crate::PlayState::Menu);
    let token = row(&app, Side::Container, GOODS);
    tap(&mut app, Target::Row(token));
    let point = at(&app, Target::Close);
    edge(&mut app, 12, TouchPhase::Started, point); app.update();
    app.world_mut().write_message(MenuInput(Command::One)); app.update();
    assert!(model(&app).quantity.is_some(), "real ordinary consumer opened a prompt");
    edge(&mut app, 12, TouchPhase::Ended, point); app.update();
    assert!(model(&app).quantity.is_some(), "old held Close cannot act on a new prompt");
    assert_eq!(offered(&app), 0);
    tap(&mut app, Target::CancelAmount);
    assert!(model(&app).quantity.is_none());
}

#[test]
fn actual_touch_barter_dialogue_close_uses_real_menu_exit_and_never_accepts_basket() {
    let (mut app, stock) = scene(true, crate::PlayState::Dialogue);
    let token = row(&app, Side::Player, GOODS);
    tap(&mut app, Target::Row(token)); tap(&mut app, Target::All);
    assert_eq!(offered(&app), 8);
    tap(&mut app, Target::Close);
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    assert_eq!(*app.world().resource::<State<crate::PlayState>>().get(), crate::PlayState::Dialogue);
    assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
}

#[test]
fn actual_touch_barter_hardware_return_and_focus_gate_retire_contact_without_changing_hardware() {
    let (mut app, _) = scene(false, crate::PlayState::Menu);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyF);
    let token = row(&app, Side::Container, GOODS);
    let point = at(&app, Target::Row(token));
    edge(&mut app, 9, TouchPhase::Started, point); app.update();
    app.world_mut().resource_mut::<TouchBarter>().hardware_intent();
    edge(&mut app, 9, TouchPhase::Ended, point); app.update();
    assert!(!app.world().resource::<TouchBarter>().visible());
    edge(&mut app, 10, TouchPhase::Started, point); app.update();
    edge(&mut app, 10, TouchPhase::Ended, point); app.update();
    assert!(app.world().resource::<TouchBarter>().visible());
    assert_eq!(model(&app).focus, Side::Player, "return finger performs no selection");
    let window = app.world().resource::<Fixture>().0.window;
    edge(&mut app, 11, TouchPhase::Started, point); app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    edge(&mut app, 11, TouchPhase::Ended, point); app.update();
    assert_eq!(model(&app).focus, Side::Player);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update(); tap(&mut app, Target::Row(token));
    assert_eq!(model(&app).focus, Side::Container);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyF));
}

#[test]
fn actual_touch_barter_rechecks_live_geometry_and_admission_after_raw_before_effects() {
    for native in [false, true] {
        for change in 0..10 {
            let (mut app, stock) = scene(native, crate::PlayState::Menu);
            let point = at(&app, Target::All);
            edge(&mut app, 20, TouchPhase::Started, point); app.update();
            edge(&mut app, 20, TouchPhase::Ended, point);
            app.world_mut().resource_mut::<LateChange>().0 = Some(change);
            app.update();
            assert_eq!(offered(&app), 0, "late admission {change}, native={native}");
            assert!(model(&app).quantity.is_none());
            assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
            for _ in 0..3 { app.update(); }
            assert_eq!(offered(&app), 0, "a discarded request never replays");
        }
    }
}

#[test]
fn actual_touch_barter_meter_drag_cannot_confirm_with_a_fresh_same_batch_finger() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::One);
        let panel = view::Panel::of(app.world().resource::<Fixture>().0).unwrap();
        let meter = panel.meter.unwrap();
        let a = Vec2::new(meter.min.x + 1.0, meter.center().y);
        let b = Vec2::new(meter.max.x - 1.0, meter.center().y);
        let confirm = at(&app, Target::ConfirmAmount);
        edge(&mut app, 30, TouchPhase::Started, a); app.update();
        edge(&mut app, 30, TouchPhase::Moved, meter.center());
        edge(&mut app, 30, TouchPhase::Moved, b);
        edge(&mut app, 30, TouchPhase::Ended, b);
        edge(&mut app, 31, TouchPhase::Started, confirm);
        edge(&mut app, 31, TouchPhase::Ended, confirm);
        app.update();
        assert_eq!(model(&app).quantity.unwrap().amount, 8);
        assert_eq!(offered(&app), 0, "scroll/drag/secondary finger never confirms quantity");
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        for _ in 0..3 { app.update(); }
        assert_eq!(offered(&app), 0);
        tap(&mut app, Target::ConfirmAmount);
        assert_eq!(offered(&app), 8, "a separate fresh confirm is required");
    }
}

#[test]
fn actual_touch_barter_contact_budget_fails_closed_then_recovers_only_after_neutral() {
    let (mut app, _) = scene(false, crate::PlayState::Menu);
    let token = row(&app, Side::Container, GOODS);
    let point = at(&app, Target::Row(token));
    for id in 100..135 { edge(&mut app, id, TouchPhase::Started, point); }
    app.update();
    assert!(app.world().resource::<TouchBarter>().overflow);
    assert_eq!(app.world().resource::<TouchBarter>().retired.len(), MAX_IDS);
    assert!(app.world().resource::<TouchBarter>().contact.is_none());
    for id in 100..135 { edge(&mut app, id, TouchPhase::Ended, point); }
    app.update();
    assert_eq!(model(&app).focus, Side::Player);
    assert_eq!(offered(&app), 0);
    assert!(app.world().resource::<TouchBarter>().overflow, "release frame stays captured");
    app.update();
    assert!(!app.world().resource::<TouchBarter>().overflow);
    tap(&mut app, Target::Row(token));
    assert_eq!(model(&app).focus, Side::Container, "fresh contact recovers after neutral");
    assert_eq!(offered(&app), 0);
}

#[test]
fn actual_touch_barter_hardware_handoff_keeps_the_original_accept_key_consumer() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Player, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::All);
        assert_eq!(offered(&app), 8);
        // The parent owns provenance. Exercise its leaf callback, then feed the
        // genuine existing keyboard pipeline; no command or effect is injected.
        app.world_mut().resource_mut::<TouchBarter>().hardware_intent();
        keyboard(&mut app, true);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (0, 16));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (1072, 928));
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyA));
        assert!(!app.world().resource::<TouchBarter>().visible());
        keyboard(&mut app, false);
        for _ in 0..3 { app.update(); }
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (0, 16));
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyA));
    }
}

#[test]
fn actual_touch_barter_late_script_message_prevents_accept_then_fresh_review_can_trade() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Player, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::All);
        assert_eq!(offered(&app), 8);
        let point = at(&app, Target::Accept);
        edge(&mut app, 40, TouchPhase::Started, point); app.update();
        edge(&mut app, 40, TouchPhase::Ended, point);
        app.world_mut().resource_mut::<LateChange>().0 = Some(10);
        app.update();
        assert_eq!(app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.top_menu(),
            Some(fnv_game::game::menus::MESSAGE), "actual script menu opened after raw input");
        assert_eq!(offered(&app), 8);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        assert_eq!(count(&mut app, PLAYER, CAPS), 1000);
        for _ in 0..3 { app.update(); }
        assert_eq!(offered(&app), 8);
        app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap()
            .game.open_menu(fnv_game::game::menus::BARTER);
        app.update();
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8), "no stale replay on modal exit");
        tap(&mut app, Target::Accept);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (0, 16));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (1072, 928));
    }
}

#[test]
fn actual_touch_barter_final_consumer_rejects_queued_amount_change_and_consumes_token_once() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::One); tap(&mut app, Target::Min);
        assert_eq!(model(&app).quantity.unwrap().amount, 1);
        let point = at(&app, Target::ConfirmAmount);
        edge(&mut app, 50, TouchPhase::Started, point); app.update();
        // Genuine existing command already queued before the touch producer.
        app.world_mut().write_message(MenuInput(Command::Amount(8)));
        edge(&mut app, 50, TouchPhase::Ended, point); app.update();
        assert_eq!(model(&app).quantity.unwrap().amount, 8, "ordinary input keeps its behavior");
        assert_eq!(offered(&app), 0, "the touch confirmation saw one, never eight");
        let envelope = app.world().resource::<LastEnvelope>().0.unwrap();
        app.world_mut().write_message(MenuInput(Command::CommitBarter(envelope))); app.update();
        assert_eq!(offered(&app), 0, "rejected envelope is consumed, not delayed");
        tap(&mut app, Target::Min); tap(&mut app, Target::ConfirmAmount);
        assert_eq!(offered(&app), 1, "fresh reviewed amount still stages one");
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        tap(&mut app, Target::Accept);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (9, 7));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (989, 1011));
    }
}

#[test]
fn actual_touch_barter_final_consumer_rejects_queued_stack_before_accept_then_fresh_quote_succeeds() {
    for native in [false, true] {
        let (mut app, stock) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::All);
        assert_eq!(model(&app).trade.as_ref().unwrap().balance, -88);
        let sell = {
            let m = model(&app);
            let index = m.list(Side::Player).rows.iter().position(|r| r.item == GOODS && !r.bartered).unwrap();
            RowToken::at(m, Side::Player, index).unwrap()
        };
        let point = at(&app, Target::Accept);
        edge(&mut app, 51, TouchPhase::Started, point); app.update();
        for command in [Command::Focus(Side::Player), Command::Hover(Side::Player, sell.index), Command::Stack] {
            app.world_mut().write_message(MenuInput(command));
        }
        edge(&mut app, 51, TouchPhase::Ended, point); app.update();
        assert_eq!(offered(&app), 16, "the ordinary Stack still changes the basket");
        assert_eq!(model(&app).trade.as_ref().unwrap().balance, -16);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (1000, 1000));
        let envelope = app.world().resource::<LastEnvelope>().0.unwrap();
        app.world_mut().write_message(MenuInput(Command::CommitBarter(envelope))); app.update();
        assert_eq!(count(&mut app, PLAYER, CAPS), 1000);
        tap(&mut app, Target::Accept);
        assert_eq!(offered(&app), 0);
        assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
        assert_eq!((count(&mut app, PLAYER, CAPS), count(&mut app, stock, CAPS)), (984, 1016));
    }
}

#[test]
fn actual_touch_barter_final_consumer_rechecks_modal_loading_and_actual_target_size() {
    for native in [false, true] {
        for change in [10, 11, 12, 13] {
            let (mut app, stock) = scene(native, crate::PlayState::Menu);
            let token = row(&app, Side::Player, GOODS);
            tap(&mut app, Target::Row(token)); tap(&mut app, Target::All);
            let point = at(&app, Target::Accept);
            edge(&mut app, 52, TouchPhase::Started, point); app.update();
            edge(&mut app, 52, TouchPhase::Ended, point);
            app.world_mut().resource_mut::<LateConsumer>().0 = Some(change);
            app.update();
            assert!(app.world().resource::<LastEnvelope>().0.is_some(), "producer emitted actual token before late change");
            assert_eq!(offered(&app), 8, "late final gate {change}, native={native}");
            assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
            assert_eq!(count(&mut app, PLAYER, CAPS), 1000);
            for _ in 0..3 { app.update(); }
            assert_eq!(count(&mut app, PLAYER, CAPS), 1000, "late refused token never retries");
        }
    }
}

#[test]
fn actual_touch_barter_duplicate_envelope_in_same_consumer_batch_steps_quantity_once() {
    for native in [false, true] {
        let (mut app, _) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::One); tap(&mut app, Target::Min);
        assert_eq!(model(&app).quantity.unwrap().amount, 1);
        let point = at(&app, Target::Step(1));
        edge(&mut app, 53, TouchPhase::Started, point); app.update();
        app.world_mut().resource_mut::<RepeatEnvelope>().next = true;
        edge(&mut app, 53, TouchPhase::Ended, point); app.update();
        assert_eq!(app.world().resource::<RepeatEnvelope>().queued, 1, "duplicate genuine token queued before consumer");
        assert_eq!(app.world().resource::<BeforeConsumerCount>().0, 2, "both real copies exist at the actual consumer boundary");
        assert_eq!(model(&app).quantity.unwrap().amount, 2, "one-shot token steps once even in the same frame");
        assert_eq!(offered(&app), 0);
        for _ in 0..3 { app.update(); }
        assert_eq!(model(&app).quantity.unwrap().amount, 2);
        tap(&mut app, Target::Step(1));
        assert_eq!(model(&app).quantity.unwrap().amount, 3, "a new genuine contact is independent");
    }
}

#[test]
fn actual_touch_barter_obsolete_token_before_new_token_preserves_current_proposal() {
    for native in [false, true] {
        let (mut app, _) = scene(native, crate::PlayState::Menu);
        let token = row(&app, Side::Container, GOODS);
        tap(&mut app, Target::Row(token)); tap(&mut app, Target::One); tap(&mut app, Target::Min);
        let obsolete = app.world().resource::<LastEnvelope>().0.unwrap();
        let point = at(&app, Target::Step(1));
        edge(&mut app, 54, TouchPhase::Started, point); app.update();
        app.world_mut().write_message(MenuInput(Command::CommitBarter(obsolete)));
        edge(&mut app, 54, TouchPhase::Ended, point); app.update();
        assert_ne!(app.world().resource::<LastEnvelope>().0, Some(obsolete));
        assert_eq!(model(&app).quantity.unwrap().amount, 2, "stale token must not consume a newer live guard");
        assert_eq!(offered(&app), 0);
        tap(&mut app, Target::Step(1));
        assert_eq!(model(&app).quantity.unwrap().amount, 3);
    }
}

#[test]
fn actual_touch_barter_all_nonspend_actions_obey_final_modal_ownership_then_fresh_input_works() {
    for native in [false, true] {
        for case in 0..5 {
            let (mut app, stock) = scene(native, crate::PlayState::Dialogue);
            let target = match case {
                0 => Target::Close, 1 => Target::One,
                2 => Target::Row(row(&app, Side::Container, GOODS)),
                3 => Target::Category(Side::Player, 1),
                _ => Target::Row(row(&app, Side::Player, GOODS)),
            };
            let point = at(&app, target);
            edge(&mut app, 55, TouchPhase::Started, point); app.update();
            if case == 4 { edge(&mut app, 55, TouchPhase::Moved, point - Vec2::Y * 70.0); }
            else { edge(&mut app, 55, TouchPhase::Ended, point); }
            app.world_mut().resource_mut::<LateConsumer>().0 = Some(10);
            app.update();
            assert!(app.world().resource::<ActiveMenu>().0.is_some(), "late Close must not close underlying barter");
            assert_eq!(model(&app).focus, Side::Player);
            assert_eq!(model(&app).category[1], Category::All);
            assert_eq!(model(&app).selected[1], 0);
            assert!(model(&app).quantity.is_none());
            assert_eq!(offered(&app), 0);
            assert_eq!((count(&mut app, PLAYER, GOODS), count(&mut app, stock, GOODS)), (8, 8));
            if case == 4 { edge(&mut app, 55, TouchPhase::Canceled, point); app.update(); }
            app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap()
                .game.open_menu(fnv_game::game::menus::BARTER);
            app.update();
            assert!(model(&app).quantity.is_none(), "no replay when modal closes");
            if case == 4 {
                edge(&mut app, 56, TouchPhase::Started, point); app.update();
                edge(&mut app, 56, TouchPhase::Moved, point - Vec2::Y * 70.0); app.update();
                edge(&mut app, 56, TouchPhase::Ended, point - Vec2::Y * 70.0); app.update();
                assert!(model(&app).selected[1] > 0);
            } else {
                tap(&mut app, target);
                match case {
                    0 => assert!(app.world().resource::<ActiveMenu>().0.is_none()),
                    1 => assert!(model(&app).quantity.is_some()),
                    2 => assert_eq!(model(&app).focus, Side::Container),
                    3 => assert_eq!(model(&app).category[1], Category::Weapons),
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[test]
fn barter_sheet_paint_and_hit_targets_share_safe_scaled_clipping_and_minimum_48_pixels() {
    let (app, _) = scene(false, crate::PlayState::Menu);
    let f = app.world().resource::<Fixture>().0;
    for invalid in [Frame { size: Vec2::ONE, safe: crate::touch_safe_area::SafeInsets {
            left: 512.0, right: 512.0, top: 512.0, bottom: 512.0 }, ..f },
        Frame { ui_scale: f32::NAN, ..f }, Frame { ui_scale: 0.0, ..f },
        Frame { ui_scale: f32::MIN_POSITIVE / 32.0, ..f }, Frame { size: Vec2::new(400.0, 200.0), ..f }] {
        assert!(view::Panel::of(invalid).is_none(), "invalid geometry cannot create a reversed/clipped hit panel");
    }
    for size in [Vec2::new(640.0, 360.0), Vec2::new(792.0, 363.0)] {
        for scale in [0.75, 1.0, 1.5, 2.0] {
            let f = Frame { size, ui_scale: scale, safe: crate::touch_safe_area::SafeInsets {
                left: 16.0, right: 8.0, top: 0.0, bottom: 12.0 }, ..app.world().resource::<Fixture>().0 };
            let p = view::Panel::of(f).unwrap();
            let hits = p.hits(model(&app), [0; 2]);
            for (i, hit) in hits.iter().enumerate() {
                assert!(hit.rect.width() >= 48.0 && hit.rect.height() >= 48.0);
                assert!(p.area.contains(hit.rect.min) && hit.rect.max.cmple(p.area.max).all());
                assert!((hit.rect.min / scale * scale).abs_diff_eq(hit.rect.min, 0.001));
                for other in &hits[i + 1..] { assert_eq!(hit.rect.intersect(other.rect).area(), 0.0, "overlapping hit targets"); }
            }
        }
    }
}

#[test]
fn actual_barter_sheet_nodes_use_the_same_current_scaled_rectangles_as_raw_hits() {
    let (mut app, _) = scene(false, crate::PlayState::Menu);
    for scale in [0.75, 1.0, 1.5, 2.0] {
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        app.world_mut().resource_mut::<Fixture>().0.ui_scale = scale;
        app.update();
        let state = app.world().resource::<TouchBarter>();
        let f = state.frame.unwrap();
        let hits = view::Panel::of(f).unwrap().hits(model(&app), state.first);
        let root = app.world_mut().query_filtered::<Entity, With<view::Sheet>>().single(app.world()).unwrap();
        assert_eq!(app.world().get::<UiTargetCamera>(root).unwrap().0, f.camera);
        let children = app.world().get::<Children>(root).unwrap();
        for hit in hits {
            assert!(children.iter().any(|entity| app.world().get::<Node>(entity).is_some_and(|node|
                node.left == px(hit.rect.min.x / scale) && node.top == px(hit.rect.min.y / scale)
                && node.width == px(hit.rect.width() / scale) && node.height == px(hit.rect.height() / scale))),
                "the actual painted node and its raw hit rectangle differ");
        }
    }
}

include!("main_preferences_tests.rs");
include!("normal_tests.rs");
include!("normal_size_tests.rs");

include!("normal_typography_tests.rs");

include!("normal_meter_tests.rs");
