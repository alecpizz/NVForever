use super::*;
use bevy::ecs::system::RunSystemOnce;
use fnv_sim::forms::{ActorBase, BaseData, BaseForm, Item, ItemKind};
use fnv_sim::{FormStore, RefTable, SimConfig};

const AID: FormId = FormId(0x100);

fn sim_with_radaway(count: i32) -> crate::sim::SimState {
    let mut forms = FormStore::default();
    let item = Item {
        kind: ItemKind::Aid, value: Some(1), money: None, weight: Some(0.1), slots: None,
        projectile: None, health: None, non_playable: false, cant_drop: false,
        armor_class: Default::default(), power_armor: false, armor_dr: None, armor_dt: None,
        weapon_damage: None, weight_mods: [0.0; 3], drawable: false, not_for_npcs: false,
        thrown: false, quest_item: false, icon: None, placed_count: None, teaches: None,
        cant_be_taken: false, modulates_voice: false, weapon_traits: None,
    };
    for (id, data) in [
        (AID, BaseData::Item(item)),
        (FormId::PLAYER_BASE, BaseData::Actor(ActorBase { entries: Vec::new(), attributes: Some([5; 7]) })),
    ] {
        forms.bases.insert(id, BaseForm {
            id, sig: if id == AID { *b"ALCH" } else { *b"NPC_" },
            edid: None, name: Some("Test aid".into()), script: None,
            prompt: None, voice_filter: false, data,
        });
    }
    // Synthetic forms; only the lookup's Editor ID is the production action's name.
    forms.edids.insert("radaway".into(), AID);
    let forms = Box::leak(Box::new(forms));
    let refs = Box::leak(Box::new(RefTable::default()));
    let mut world = fnv_sim::World::new(forms, refs, SimConfig::default());
    if count > 0 {
        world.add_item(FormId::PLAYER_REF, AID, count).unwrap();
    }
    crate::sim::SimState { forms, refs, world }
}

/// The Pip-Boy's tab keys, then the page's own [`prepare`] (which routes the view's keys),
/// as the plugin chains them; one frame with nothing pressed has shown the page already.
fn input_app(status: usize, tab: usize) -> App {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<super::super::KeysTaken>()
        .init_resource::<crate::menu::PromptKeys>()
        .init_resource::<crate::menu::ActiveMenu>()
        .insert_resource(Time::<()>::default())
        .init_resource::<crate::pause::PlayClock>()
        .init_resource::<Prepared>()
        .insert_resource(sim_with_radaway(0))
        .insert_resource(PipBoy { page: Page::Stats, phase: super::super::Phase::Up, tabs: [tab, 0, 0], ..default() })
        .insert_resource(StatsView { status, ..default() })
        .add_message::<super::super::PipBoyInput>()
        .add_message::<UseHint>()
        .add_message::<MouseWheel>()
        .add_systems(Update, (super::super::keys, super::super::apply_input, prepare).chain());
    app.update();
    app
}

#[test]
fn rad_a_uses_one_carried_radaway_and_keeps_status_even_when_empty() {
    for count in [0, 2] {
        let mut app = input_app(1, 0);
        app.insert_resource(sim_with_radaway(count))
            .add_message::<crate::ItemUsed>()
            .add_message::<crate::ui_sounds::UiSound>()
            .add_systems(Update, use_hints.after(prepare));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyA);
        app.update();
        assert_eq!(app.world().resource::<PipBoy>().tab(), 0);
        assert_eq!(app.world().resource::<StatsView>().status, 1);
        let sim = &mut *app.world_mut().resource_mut::<crate::sim::SimState>();
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF, AID).unwrap(), (count - 1).max(0));
        let used: Vec<_> = app.world_mut().resource_mut::<Messages<crate::ItemUsed>>().drain().collect();
        assert_eq!(used.len(), usize::from(count > 0));
        if let Some(used) = used.first() {
            assert_eq!((used.item, used.limb), (AID.0, None));
        }
    }
}

#[test]
fn tab_keys_switch_every_stats_view_that_does_not_own_them() {
    use KeyCode::*;
    for status in 0..3 {
        for tab in 0..5 {
            for (key, by) in [(KeyA, 4), (ArrowLeft, 4), (KeyD, 1), (ArrowRight, 1)] {
                let mut app = input_app(status, tab);
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
                app.update();
                let rad = status == 1 && tab == 0 && key == KeyA;
                let expected = if rad { 0 } else { (tab + by) % 5 };
                assert_eq!(app.world().resource::<PipBoy>().tab(), expected, "status {status} tab {tab} key {key:?}");
                // A press that switched the tab is spent: it uses nothing on the new tab
                // (A from S.P.E.C.I.A.L. onto a Status left on RAD, #1899).
                let asks: Vec<_> = app.world_mut().resource_mut::<Messages<UseHint>>().drain().collect();
                let used: &[UseHint] = if rad { &[UseHint { hint: model::RADAWAY, limb: None }] } else { &[] };
                assert_eq!(asks, used, "status {status} tab {tab} key {key:?}");
            }
        }
    }
}

#[test]
fn a_tutorial_over_rad_holds_a_through_the_frame_it_closes_in() {
    // As the plugin gates the Pip-Boy's own keys (`takes_keys`); the page's run regardless.
    let mut state = crate::script::ScriptState::over_no_plugins();
    state.rt.as_mut().unwrap().game.menu = Some(fnv_game::game::menus::TUTORIAL);
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<super::super::KeysTaken>()
        .init_resource::<crate::menu::PromptKeys>()
        .init_resource::<crate::menu::ActiveMenu>()
        .insert_resource(Time::<()>::default())
        .init_resource::<crate::pause::PlayClock>()
        .init_resource::<Prepared>()
        .insert_resource(sim_with_radaway(0))
        .insert_resource(state)
        .insert_resource(PipBoy { page: Page::Stats, phase: super::super::Phase::Up, tabs: [0, 0, 0], ..default() })
        .insert_resource(StatsView { status: 1, ..default() })
        .add_message::<super::super::PipBoyInput>()
        .add_message::<UseHint>()
        .add_message::<MouseWheel>()
        .add_systems(Update, ((super::super::keys, super::super::apply_input).chain().run_if(super::super::takes_keys), prepare).chain());
    app.update();
    let press_a = |app: &mut App| {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::KeyA);
    };
    let asks = |app: &mut App| app.world_mut().resource_mut::<Messages<UseHint>>().drain().collect::<Vec<_>>();
    // The tutorial is up: A is its Next.
    press_a(&mut app);
    app.update();
    assert_eq!(asks(&mut app), [], "A under the tutorial uses no RadAway");
    // The frame the tutorial closes in: its key answered it before the page's keys ran.
    app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.menu = None;
    press_a(&mut app);
    app.update();
    assert_eq!(asks(&mut app), [], "the press that closed the tutorial is spent");
    assert_eq!((app.world().resource::<PipBoy>().tab(), app.world().resource::<StatsView>().status), (0, 1));
    // The next press is the page's again.
    press_a(&mut app);
    app.update();
    assert_eq!(asks(&mut app), [UseHint { hint: model::RADAWAY, limb: None }]);
}

#[test]
fn cnd_pad_x_cycles_all_limbs_and_wraps_to_the_body() {
    let at = crate::pad::MenuAt { code: Some(fnv_game::game::menus::STATS), tab: 0 };
    let key = crate::pad::menu_buttons(at).iter().find(|(b, _)| *b == GamepadButton::West).unwrap().1;
    let mut view = StatsView::default();
    for expected in (0..6).map(Some).chain([None, Some(0)]) {
        let mut input = ButtonInput::default();
        input.press(key);
        assert_eq!(keys(&input, &mut view, 0, false), None);
        assert_eq!(view.limb, expected);
        assert_eq!(view.aimed(), expected);
        assert_eq!(hint_pressed(&view, model::STIMPAK, false).unwrap().limb, expected);
        assert_eq!(hint_pressed(&view, model::DOCTORS_BAG, true).unwrap().limb, expected);
    }
    // A pointer left on a limb must not mask the new choice.
    view.hover = Some(Spot::Limb(2));
    view.cycle_limb();
    assert_eq!(view.aimed(), Some(3));
    for (tab, status) in [(0, 1), (0, 2), (1, 0), (2, 0), (3, 0), (4, 0)] {
        let mut view = StatsView { status, ..default() };
        let mut input = ButtonInput::default();
        input.press(key);
        keys(&input, &mut view, tab, false);
        assert_eq!(view.limb, None);
    }
}

#[test]
fn hardcore_cnd_selects_limbs_for_doctors_bag_and_keeps_body_when_none_aimed() {
    let mut view = StatsView::default();
    // Selecting a limb via digit 3 (left arm, index 2) and pressing E targets that limb in Hardcore.
    let mut input = ButtonInput::default();
    input.press(KeyCode::Digit3);
    assert_eq!(keys(&input, &mut view, 0, true), None);
    assert_eq!(view.aimed(), Some(2));
    let mut input = ButtonInput::default();
    input.press(KeyCode::KeyE);
    assert_eq!(keys(&input, &mut view, 0, true), Some(UseHint { hint: model::DOCTORS_BAG, limb: Some(2) }));
    // Outside Hardcore, Doctor's Bag still heals the whole body even with a limb chosen.
    assert_eq!(keys(&input, &mut view, 0, false), Some(UseHint { hint: model::DOCTORS_BAG, limb: None }));
    // Hovering another limb overrides keyboard choice; pressing digit 3 again clears hover and toggles back to body.
    view.hover = Some(Spot::Limb(5));
    assert_eq!(keys(&input, &mut view, 0, true), Some(UseHint { hint: model::DOCTORS_BAG, limb: Some(5) }));
    let mut toggle = ButtonInput::default();
    toggle.press(KeyCode::Digit3);
    keys(&toggle, &mut view, 0, true);
    assert_eq!(view.aimed(), None);
    assert_eq!(keys(&input, &mut view, 0, true), Some(UseHint { hint: model::DOCTORS_BAG, limb: None }));
}

#[test]
fn hardcore_use_hints_targets_selected_hurt_limb_and_refuses_unhurt_limb() {
    const DRBAG: FormId = FormId(0x101);
    let mut forms = FormStore::default();
    let item = Item {
        kind: ItemKind::Aid, value: Some(55), money: None, weight: Some(1.0), slots: None,
        projectile: None, health: None, non_playable: false, cant_drop: false,
        armor_class: Default::default(), power_armor: false, armor_dr: None, armor_dt: None,
        weapon_damage: None, weight_mods: [0.0; 3], drawable: false, not_for_npcs: false,
        thrown: false, quest_item: false, icon: None, placed_count: None, teaches: None,
        cant_be_taken: false, modulates_voice: false, weapon_traits: None,
    };
    for (id, data) in [
        (DRBAG, BaseData::Item(item)),
        (FormId::PLAYER_BASE, BaseData::Actor(ActorBase { entries: Vec::new(), attributes: Some([5; 7]) })),
    ] {
        forms.bases.insert(id, BaseForm {
            id, sig: if id == DRBAG { *b"ALCH" } else { *b"NPC_" },
            edid: None, name: Some("Doctor's Bag".into()), script: None,
            prompt: None, voice_filter: false, data,
        });
    }
    forms.edids.insert("nvdoctorbag".into(), DRBAG);
    let forms = Box::leak(Box::new(forms));
    let refs = Box::leak(Box::new(RefTable::default()));
    let mut world = fnv_sim::World::new(forms, refs, SimConfig::default());
    world.add_item(FormId::PLAYER_REF, DRBAG, 2).unwrap();
    let sim = crate::sim::SimState { forms, refs, world };

    let mut script = crate::script::ScriptState::over_no_plugins();
    let g = &mut script.rt.as_mut().unwrap().game;
    g.set_hardcore(true);
    fnv_game::functions::add_actor_value_damage(g, FormId::PLAYER_REF, model::LIMBS[2], 100.0);

    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<super::super::KeysTaken>()
        .init_resource::<crate::menu::PromptKeys>()
        .init_resource::<crate::menu::ActiveMenu>()
        .insert_resource(Time::<()>::default())
        .init_resource::<crate::pause::PlayClock>()
        .init_resource::<Prepared>()
        .insert_resource(sim)
        .insert_resource(script)
        .insert_resource(PipBoy { page: Page::Stats, phase: super::super::Phase::Up, tabs: [0, 0, 0], ..default() })
        .insert_resource(StatsView { status: 0, ..default() })
        .add_message::<super::super::PipBoyInput>()
        .add_message::<UseHint>()
        .add_message::<MouseWheel>()
        .add_message::<crate::ItemUsed>()
        .add_message::<crate::ui_sounds::UiSound>()
        .add_systems(Update, (prepare, use_hints).chain());
    // Frame 1: open page (first frame is `new`).
    app.update();
    // Frame 2: select right arm (digit 4 -> index 3, which is whole) and press E -> refused, 2 bags remain.
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Digit4);
        keys.press(KeyCode::KeyE);
    }
    app.update();
    let used: Vec<_> = app.world_mut().resource_mut::<Messages<crate::ItemUsed>>().drain().collect();
    assert!(used.is_empty(), "unhurt limb must not consume a Doctor's Bag");
    assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF, DRBAG).unwrap(), 2);
    // Frame 3: select left arm (digit 3 -> index 2, crippled) and press E -> uses 1 bag on LeftAttackCondition (27).
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Digit3);
        keys.press(KeyCode::KeyE);
    }
    app.update();
    let used: Vec<_> = app.world_mut().resource_mut::<Messages<crate::ItemUsed>>().drain().collect();
    assert_eq!(used.len(), 1);
    assert_eq!((used[0].item, used[0].limb), (DRBAG.0, Some(model::LIMBS[2])));
    assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF, DRBAG).unwrap(), 1);
}

#[test]
fn cnd_draws_three_hints_in_the_existing_slots() {
    // The page lays out in the Pip-Boy screen's own units, as `prepare` draws it (#1631).
    let _canvas = crate::interface::original_units();
    let mut world = World::new();
    world.run_system_once(|mut commands: Commands| {
        let mut kit = Kit::default();
        let mut images = Assets::<Image>::default();
        let glyphs = crate::pad::PadGlyphs::default();
        let mut c = Ctx::new(&mut kit, None, &mut images, None, &glyphs);
        let s = Snapshot { switch_limbs: "Test target".into(), ..default() };
        commands.spawn(Node::default()).with_children(|p| status(p, &mut c, &mut StatsView::default(), &s));
    }).unwrap();
    let mut hints: Vec<_> = world.query::<(&Spot, &Node)>().iter(&world).filter_map(|(spot, node)| {
        if let Spot::Hint(i) = spot { Some((*i, node.top)) } else { None }
    }).collect();
    hints.sort_by_key(|(i, _)| *i);
    assert_eq!(hints.len(), 3);
    let top = |i| hints.iter().find(|(hint, _)| *hint == i).unwrap().1;
    // Spots are placed in interface units (`crate::interface::at`, a part of the view's
    // height), not pixels.
    let (Val::Vh(first), Val::Vh(middle), Val::Vh(last)) = (top(0), top(SWITCH_LIMBS), top(1)) else { panic!("hint positions must be interface units") };
    // The first hint sits in the page's first hint slot, the others one slot below each.
    assert_eq!(Val::Vh(first), crate::interface::u(STATUS.min.y + HINT_Y));
    assert!(first < middle && middle < last);
    assert!((middle - first - (last - middle)).abs() < 1e-4, "{first} {middle} {last}");
    for (labels, expected) in [(crate::pad::Labels::Keys, ["[S]", "[X]", "[E]"]), (crate::pad::Labels::Xbox, ["[A]", "[X]", "[Y]"]), (crate::pad::Labels::PlayStation, ["[Cross]", "[Square]", "[Triangle]"])] {
        crate::pad::set_labels(labels);
        assert_eq!([hint_key(&StatsView::default(), 0), hint_key(&StatsView::default(), SWITCH_LIMBS), hint_key(&StatsView::default(), 1)], expected);
    }
    crate::pad::set_labels(crate::pad::Labels::Keys);
}

#[test]
fn projected_mobile_scroll_uses_real_stats_view_bounds_and_does_not_use_a_hint(){
    let mut app=input_app(0,2);
    {let mut view=app.world_mut().resource_mut::<StatsView>();view.lens[2]=20;view.set_rows_shown(2,5);}
    let mut scroll=super::super::touch_scroll::WheelFrame::default();
    scroll.set(app.world().resource::<PipBoy>(),Vec2::new(40.0,100.0),-60.0);app.insert_resource(scroll);app.update();
    assert_eq!(app.world().resource::<StatsView>().top[2],3);
    assert!(app.world().resource::<Messages<UseHint>>().is_empty());
}

#[test]
fn physical_mouse_and_wheel_keep_their_description_target_and_a_later_pure_touch_scrolls_the_list(){
    use crate::pipboy::{self,touch_items::TouchItems,touch_scroll::{TouchPointerPlugin,WheelFrame,PhysicalPriority}};
    use bevy::input::touch::{TouchInput,TouchPhase};
    fn project(mut touch:ResMut<TouchItems>){touch.project(Some);}
    fn finger(app:&mut App,phase:TouchPhase,at:Vec2,id:u64){assert!(app.world_mut().resource_mut::<TouchItems>().event(&TouchInput{window:Entity::PLACEHOLDER,id,phase,position:at,force:None},0));}
    for cause in 0..7 {
        let mut app=input_app(0,2);let desc=desc_rect(2).unwrap().center();let list=Vec2::new(40.,100.);
        {let mut view=app.world_mut().resource_mut::<StatsView>();view.lens[2]=20;view.set_rows_shown(2,5);view.set_desc_bound(100);view.desc=10;}
        app.init_resource::<TouchItems>().init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>();
        let camera=app.world_mut().spawn_empty().id();app.insert_resource(pipboy::PipBoyScreen{image:Handle::default(),camera,on_model:false,pointer:Some(desc),scale:1.0});
        app.add_plugins(TouchPointerPlugin).add_systems(Last,project);app.update();
        if cause!=6 {
            finger(&mut app,TouchPhase::Started,list,201);app.update();app.update();
            finger(&mut app,TouchPhase::Moved,list-Vec2::Y*60.,201);app.update();
            finger(&mut app,TouchPhase::Ended,list-Vec2::Y*60.,201);
        }
        match cause {
            0=>app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left),
            1=>app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right),
            2=>{let mut m=app.world_mut().resource_mut::<ButtonInput<MouseButton>>();m.press(MouseButton::Left);m.release(MouseButton::Left);},
            3=>{let mut m=app.world_mut().resource_mut::<ButtonInput<MouseButton>>();m.press(MouseButton::Left);m.clear();m.release(MouseButton::Left);},
            4|6=>{app.world_mut().write_message(MouseWheel{window:Entity::PLACEHOLDER,unit:MouseScrollUnit::Line,x:0.,y:1.,phase:TouchPhase::Moved});},
            _=>{}
        }
        app.update();
        if cause!=5 {assert!(app.world().resource::<PhysicalPriority>().active);assert_eq!(app.world().resource::<WheelFrame>().lines(app.world().resource::<PipBoy>()),0.);}
        assert_eq!(app.world().resource::<StatsView>().top[2],if cause==5 {3}else{0});
        assert_eq!(app.world().resource::<StatsView>().desc,if cause==4||cause==6 {9}else{10});
        if cause==0 {assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));}
        if cause==1 {assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Right));}
        if cause==3 {assert!(app.world().resource::<ButtonInput<MouseButton>>().just_released(MouseButton::Left));}
        {let mut mouse=app.world_mut().resource_mut::<ButtonInput<MouseButton>>();mouse.release(MouseButton::Left);mouse.release(MouseButton::Right);mouse.clear();}
        app.update();app.update();
        let before=app.world().resource::<StatsView>().top[2];
        finger(&mut app,TouchPhase::Started,list,202);finger(&mut app,TouchPhase::Moved,list-Vec2::Y*60.,202);finger(&mut app,TouchPhase::Ended,list-Vec2::Y*60.,202);
        for _ in 0..5{app.update();}
        assert_eq!(app.world().resource::<StatsView>().top[2],before+3,"pure touch resumes with a fresh source-owned contact");
        assert_eq!(app.world().resource::<StatsView>().desc,if cause==4||cause==6 {9}else{10});
    }
}
