// Actual parent reader and deferred menu-sheet replay. No second raw reader.
mod menu_sheet_router_controls {
    use super::*;
    use nvf_play::{menu::{self, barter_touch, normal_touch, model::{Command, MenuModel}}, ui_pointer::{self, PointerState}};

    #[derive(Resource, Default)]
    struct Seen { envelopes: usize, underlay_presses: usize }
    #[derive(Component)]
    struct Underlay;
    type PaintedRoots<'w,'s> = Query<'w,'s,(Entity,Option<&'static GlobalZIndex>),(With<Node>,Without<ChildOf>)>;
    fn painted_ui_stack(mut stack:ResMut<bevy::ui::UiStack>,roots:PaintedRoots<'_,'_>,
        children:Query<&Children>,nodes:Query<Entity,With<Node>>) {
        // The lean App uses the actual painter hierarchy after layout. Each root
        // retains its own camera partition; this is not a renderer-stack test.
        fn walk(entity:Entity,children:&Query<&Children>,nodes:&Query<Entity,With<Node>>,out:&mut Vec<Entity>) {
            if nodes.get(entity).is_err(){return}
            out.push(entity);
            if let Ok(kids)=children.get(entity){for child in kids{walk(*child,children,nodes,out);}}
        }
        let mut ordered:Vec<_>=roots.iter().map(|(e,z)|(z.map_or(0,|z|z.0),e)).collect();
        ordered.sort_by_key(|(z,e)|(*z,e.to_bits()));
        stack.uinodes.clear();stack.partition.clear();
        for (_,root) in ordered {
            let start=stack.uinodes.len();
            walk(root,&children,&nodes,&mut stack.uinodes);
            let end=stack.uinodes.len();
            stack.partition.push(start..end);
        }
    }
    fn observe(mut input: MessageReader<menu::MenuInput>,
        buttons: Query<&PointerState, (With<Underlay>, Changed<PointerState>)>, mut seen: ResMut<Seen>) {
        seen.envelopes += input.read().filter(|input| matches!(input.0, Command::CommitNormalBarter(_))).count();
        seen.underlay_presses += buttons.iter().filter(|state| **state == PointerState::Pressed).count();
    }
    fn authored_barter() -> (nvf_play::sim::SimState, MenuModel) {
        use fnv_esm::{raw::{Subrecord, write_subrecords}, LoadOrder, Plugin, Schema};
        use fnv_sim::{FormId, FormStore, RefTable, SimConfig};
        fn sub(sig: &[u8;4], data: Vec<u8>) -> Subrecord { Subrecord { sig:*sig, data, extended:false } }
        fn z(text:&str)->Vec<u8>{[text.as_bytes(),&[0]].concat()}
        fn record(sig:&[u8;4], id:u32, fields:Vec<Subrecord>)->Vec<u8>{
            let data=write_subrecords(&fields);
            [&sig[..],&(data.len() as u32).to_le_bytes(),&[0;4],&id.to_le_bytes(),&[0;4],&15u16.to_le_bytes(),&[0;2],&data].concat()
        }
        fn top(sig:&[u8;4], body:Vec<u8>)->Vec<u8>{
            [b"GRUP".as_slice(),&((24+body.len()) as u32).to_le_bytes(),sig,&0i32.to_le_bytes(),&[0;8],&body].concat()
        }
        let misc=|(id,name,value):(u32,&str,i32)|record(b"MISC",id,vec![sub(b"FULL",z(name)),
            sub(b"DATA",[value.to_le_bytes(),1f32.to_le_bytes()].concat())]);
        let coins=FormId(0xf);let item=FormId(0xfa20);let stock_base=FormId(0xfa21);
        let bytes=[record(b"TES4",0,vec![sub(b"HEDR",[1.34f32.to_le_bytes(),0i32.to_le_bytes(),0x10000u32.to_le_bytes()].concat())]),
            top(b"MISC",[misc((coins.0,"Coins",1)),misc((item.0,"Authored supplies",10))].concat()),
            top(b"CONT",record(b"CONT",stock_base.0,vec![sub(b"FULL",z("Authored merchant")),sub(b"DATA",[&[0u8][..],&0f32.to_le_bytes()].concat())])),
            top(b"NPC_",record(b"NPC_",7,vec![sub(b"DATA",[&100i32.to_le_bytes()[..],&[5;7]].concat())]))].concat();
        let plugins=[Plugin::parse(&bytes,"Router.esm".into()).unwrap()];
        let order=LoadOrder::new(&plugins).unwrap();
        let schema=Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap();
        let forms=Box::leak(Box::new(FormStore::build(&order,&schema)));
        assert!(forms.issues.is_empty(),"{:?}",forms.issues.to_json());
        let refs=Box::leak(Box::new(RefTable::build(&order,&schema)));
        let mut world=fnv_sim::World::new(forms,refs,SimConfig::default());
        let stock=world.place(stock_base,None,[0.;3],[0.;3],1).unwrap();
        for holder in [FormId::PLAYER_REF,stock] {world.add_item(holder,coins,1000).unwrap();world.add_item(holder,item,8).unwrap();}
        let m=MenuModel::open_barter(&mut world,stock,stock,coins,100.,0).unwrap();
        (nvf_play::sim::SimState {forms,refs,world},m)
    }
    fn scene_with_sheet(dialogue:bool)->(App,Entity,Entity) {
        let (mut app,window)=default_menu_touch_scene(nvf_play::pipboy::Page::Items,false);
        let camera=app.world().resource::<nvf_play::pipboy::PipBoyScreen>().camera;
        app.world_mut().resource_mut::<nvf_play::pipboy::PipBoy>().phase=nvf_play::pipboy::Phase::Down;
        // The ordinary menu opens over an existing dialogue. Do not install
        // MenuPlugin, insert barter, then exit Menu and let close_menu remove it.
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(if dialogue {nvf_play::PlayState::Dialogue}else{nvf_play::PlayState::Menu});
        app.world_mut().run_schedule(bevy::state::state::StateTransition);
        let (sim,m)=authored_barter();app.insert_resource(sim).insert_resource(menu::ActiveMenu(Some(m)));
        app.world_mut().entity_mut(camera).insert(bevy::ui::IsDefaultUiCamera);
        app.insert_resource(nvf_play::Location {world:0,interior:None})
            .init_resource::<nvf_play::pause::PlayClock>()
            .init_resource::<nvf_play::pad::PadGlyphs>().init_resource::<Assets<Image>>()
            .add_message::<nvf_play::OpenContainer>().add_message::<nvf_play::pickpocket::OpenPickpocket>()
            .add_message::<nvf_play::RefChanged>().add_message::<nvf_play::ItemUsed>()
            .add_message::<nvf_play::ui_sounds::UiSound>().add_message::<nvf_play::crime::ItemsTaken>()
            .add_message::<nvf_play::pickpocket::Pickpocketed>()
            .add_plugins(menu::MenuPlugin);
        barter_touch::register_settings(&mut app);
        app.add_systems(PostUpdate,(bevy::ui::update::update_clipping_system,ApplyDeferred,painted_ui_stack)
            .chain().after(bevy::ui::ui_layout_system));
        let row=app.world().resource::<nvf_play::settings::Settings>().index(barter_touch::KEY).unwrap();
        if !(app.world().resource::<nvf_play::settings::Settings>().list()[row].get)(app.world()) {
            assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(true));
        }
        app.insert_resource(nvf_play::dialogue_touch::TouchDialogue {enabled:true,..default()});
        app.init_resource::<Seen>().init_resource::<bevy::ui::UiStack>().add_plugins(ui_pointer::UiPointerPlugin)
            .add_systems(Update,observe.after(normal_touch::Dispatch));
        for _ in 0..5 {app.update();}
        assert!(app.world().resource::<normal_touch::NormalBarter>().visible());
        assert!(!app.world().resource::<barter_touch::TouchBarter>().visible(),"historical replacement presentation is inactive");
        assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(),if dialogue {nvf_play::PlayState::Dialogue}else{nvf_play::PlayState::Menu});
        (app,window,camera)
    }
    fn painted_footer(app:&mut App)->Vec2 {
        let world=app.world_mut();
        let mut q=world.query::<(Entity,&Text)>();
        let label=q.iter(world).find_map(|(entity,text)|text.0.starts_with("Authored supplies").then_some(entity))
            .expect("real ordinary menu row label");
        let mut tile=label;
        while world.get::<ui_pointer::Button>(tile).is_none() {
            tile=world.get::<ChildOf>(tile).expect("painted row has its actual button ancestor").parent();
        }
        assert!(world.resource::<bevy::ui::UiStack>().uinodes.contains(&tile),
            "the actual stack producer published this current painted row");
        let node=world.get::<ComputedNode>(tile).unwrap();
        assert!(node.size().min_element()>=48.,"actual laid-out ordinary row has usable hit dimensions");
        let physical=world.get::<bevy::ui::UiGlobalTransform>(tile).unwrap().translation;
        let mut windows=world.query_filtered::<&Window,With<PrimaryWindow>>();
        physical/windows.single(world).unwrap().scale_factor()
    }
    fn underlay(app:&mut App,camera:Entity,point:Vec2)->Entity {
        let button=app.world_mut().spawn((Underlay,ui_pointer::Button,UiTargetCamera(camera),GlobalZIndex(i32::MAX),
            Node {position_type:PositionType::Absolute,left:px(point.x-24.),top:px(point.y-24.),width:px(48.),height:px(48.),..default()})).id();
        for _ in 0..3 {app.update();}
        assert!(app.world().get::<ComputedNode>(button).unwrap().size().min_element()>=48.);
        // The real stack producer keeps the deliberate focus probe above the sheet:
        // ownership must suppress its touch, not rely on the sheet's z-order.
        let stack=app.world().resource::<bevy::ui::UiStack>();
        assert_eq!(stack.uinodes.last(),Some(&button));
        assert!(stack.partition.iter().any(|range|stack.uinodes[range.clone()].contains(&button)));
        button
    }
    fn raw_tap(app:&mut App,window:Entity,id:u64,point:Vec2){
        touch(app,window,id,TouchPhase::Started,point.x,point.y);app.update();
        touch(app,window,id,TouchPhase::Ended,point.x,point.y);app.update();
    }
    #[test]
    fn sole_raw_sheet_router_barter_in_dialogue_precedes_dialogue_and_real_underlay_focus() {
        let (mut app,window,camera)=scene_with_sheet(true);let point=painted_footer(&mut app);
        let button=underlay(&mut app,camera,point);
        assert!(app.world().resource::<modal::Capture>().blocking(),"real Dialogue capture is active underneath barter");
        touch(&mut app,window,4100,TouchPhase::Started,point.x,point.y);app.update();
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4100));
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
        assert!(app.world().resource::<TouchState>().ui_capture);
        assert!(!app.world().resource::<nvf_play::control::HeldSneakInput>().active);
        assert!(!app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().owns(4100));
        assert_ne!(*app.world().get::<PointerState>(button).unwrap(),PointerState::Pressed);
        touch(&mut app,window,4100,TouchPhase::Ended,point.x,point.y);app.update();
        for _ in 0..4 {app.update();}
        assert_eq!(app.world().resource::<Seen>().envelopes,1,"one real guarded MenuInput reaches the existing consumer boundary");
        assert_eq!(app.world().resource::<Seen>().underlay_presses,0);
        let generic=app.world().resource::<DefaultMenuTouchSeen>();
        assert_eq!((generic.presses,generic.releases,generic.scrolls),(0,0,0));
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
    }
    #[test]
    fn sole_raw_sheet_router_keeps_ordinary_unowned_window_button_touch_enabled() {
        let (mut app,window,camera)=scene_with_sheet(false);let point=painted_footer(&mut app);
        app.world_mut().resource_mut::<menu::ActiveMenu>().0=None;
        for _ in 0..5 {app.update();}
        assert!(!app.world().resource::<normal_touch::NormalBarter>().visible());
        let button=underlay(&mut app,camera,point);
        touch(&mut app,window,4101,TouchPhase::Started,point.x,point.y);app.update();
        assert!(app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4101));
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(),PointerState::Pressed);
        touch(&mut app,window,4101,TouchPhase::Ended,point.x,point.y);app.update();
        assert_eq!(app.world().resource::<Seen>().underlay_presses,1);
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
    }
    #[test]
    fn sole_raw_sheet_router_edge_overflow_drops_whole_batch_then_neutral_accepts_fresh_contact() {
        let (mut app,window,_)=scene_with_sheet(false);let point=painted_footer(&mut app);
        touch(&mut app,window,4102,TouchPhase::Started,point.x,point.y);
        for _ in 0..129 {touch(&mut app,window,4102,TouchPhase::Moved,point.x,point.y);}
        app.update();
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4102));
        touch(&mut app,window,4102,TouchPhase::Ended,point.x,point.y);app.update();
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4102),"release frame remains quarantined");
        for _ in 0..6 {app.update();}
        assert!(app.world().resource::<normal_touch::NormalBarter>().visible());
        let point=painted_footer(&mut app);raw_tap(&mut app,window,4103,point);
        assert_eq!(app.world().resource::<Seen>().envelopes,1,"fresh contact after actual neutral recovers");
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
        assert!(!app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().busy());
    }

    #[test]
    fn sole_raw_sheet_router_actual_mouse_yields_sheet_then_first_touch_only_restores_it() {
        let (mut app,window,camera)=scene_with_sheet(false);let point=painted_footer(&mut app);
        let button=underlay(&mut app,camera,point);
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(point));
        app.world_mut().write_message(MouseButtonInput {window,button:MouseButton::Left,state:ButtonState::Pressed});
        app.update();
        assert!(app.world().resource::<TouchState>().hardware_intent);
        assert!(!app.world().resource::<normal_touch::NormalBarter>().visible());
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(),PointerState::Pressed);
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        app.world_mut().write_message(MouseButtonInput {window,button:MouseButton::Left,state:ButtonState::Released});
        app.update();app.update();
        assert_eq!(app.world().resource::<Seen>().underlay_presses,1);
        assert!(!app.world().resource::<normal_touch::NormalBarter>().visible());
        raw_tap(&mut app,window,4110,point);
        assert!(app.world().resource::<normal_touch::NormalBarter>().visible());
        assert_eq!(app.world().resource::<Seen>().envelopes,0,"return finger performs no barter action");
        assert_eq!(app.world().resource::<Seen>().underlay_presses,1,"return finger cannot click old underlay");
        for _ in 0..3 {app.update();}
        let point=painted_footer(&mut app);raw_tap(&mut app,window,4111,point);
        assert_eq!(app.world().resource::<Seen>().envelopes,1);
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }

    #[test]
    fn sole_raw_sheet_router_consumed_controller_return_restores_without_an_extra_tap() {
        let (mut app,window,camera)=scene_with_sheet(false);let point=painted_footer(&mut app);
        let button=underlay(&mut app,camera,point);
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching=true;
        nvf_play::bindings::set_device(app.world_mut(),nvf_play::bindings::InputDevice::Controller);
        for _ in 0..3 {app.update();}
        assert!(!app.world().resource::<normal_touch::NormalBarter>().visible());
        touch(&mut app,window,4112,TouchPhase::Started,point.x,point.y);app.update();
        assert_eq!(app.world().resource::<Bindings>().device,nvf_play::bindings::InputDevice::KeyboardMouse);
        assert!(!app.world().resource::<nvf_play::pad::TouchDeviceSwitch>().allows_actions());
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4112));
        assert_ne!(*app.world().get::<PointerState>(button).unwrap(),PointerState::Pressed);
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
        touch(&mut app,window,4112,TouchPhase::Ended,point.x,point.y);app.update();
        for _ in 0..4 {app.update();}
        assert!(app.world().resource::<normal_touch::NormalBarter>().visible());
        let point=painted_footer(&mut app);raw_tap(&mut app,window,4113,point);
        assert_eq!(app.world().resource::<Seen>().envelopes,1,"next fresh finger acts, with no second restore tap");
        assert_eq!(app.world().resource::<Seen>().underlay_presses,0);
    }

    #[test]
    fn sole_raw_sheet_router_pause_and_real_editor_outrank_barter_then_fresh_contact_works() {
        for editor in [false,true] {
            let (mut app,window,_)=scene_with_sheet(false);let point=painted_footer(&mut app);
            touch(&mut app,window,4114,TouchPhase::Started,point.x,point.y);app.update();
            if editor {app.world_mut().resource_mut::<Layout>().open(false);}
            else {app.insert_resource(nvf_play::pause::Paused(true));}
            touch(&mut app,window,4114,TouchPhase::Ended,point.x,point.y);app.update();
            assert_eq!(app.world().resource::<Seen>().envelopes,0);
            assert!(!app.world().resource::<normal_touch::NormalBarter>().visible());
            if editor {
                assert!(app.world().resource::<Layout>().editor.is_some());
                let cancel=layout::editor_rect(10,Vec2::splat(1000.)).center();
                raw_tap(&mut app,window,4117,cancel);
                assert!(app.world().resource::<Layout>().editor.is_none(),"actual editor Cancel returns ownership");
            } else {app.world_mut().resource_mut::<nvf_play::pause::Paused>().0=false;}
            for _ in 0..6 {app.update();}
            assert_eq!(app.world().resource::<Seen>().envelopes,0,"closing priority owner cannot replay old sheet release");
            let point=painted_footer(&mut app);raw_tap(&mut app,window,4115,point);
            assert_eq!(app.world().resource::<Seen>().envelopes,1);
        }
    }

    fn empty_play_world()->nvf_play::PlayWorld {
        let plugins=Box::leak(Vec::<fnv_esm::Plugin>::new().into_boxed_slice());
        let lo=Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
        let schema=Box::leak(Box::new(fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap()));
        let index=Box::leak(Box::new(fnv_world::WorldIndex::build(lo,schema)));
        nvf_play::PlayWorld(nvf_play::PlayData {lo,schema,index,world:0,interior:None,eye:[0.;3],heading_deg:0.,pitch_deg:0.,
            auto_walk:0.,start_at_cell_spot:false,packages:false})
    }
    fn actual_menu_button(app:&mut App,label:&str)->(Entity,Vec2) {
        let world=app.world_mut();let mut texts=world.query::<(Entity,&Text)>();
        let mut entity=texts.iter(world).find_map(|(entity,text)|(text.0==label).then_some(entity)).expect("actual MainMenu label");
        for _ in 0..16 {
            if world.get::<ui_pointer::Button>(entity).is_some() {
                let node=world.get::<ComputedNode>(entity).unwrap();assert!(node.size().min_element()>0.);
                let physical=world.get::<bevy::ui::UiGlobalTransform>(entity).unwrap().translation;
                let mut windows=world.query_filtered::<&Window,With<PrimaryWindow>>();
                return (entity,physical/windows.single(world).unwrap().scale_factor());
            }
            entity=world.get::<ChildOf>(entity).expect("label remains under actual entry button").parent();
        }
        panic!("MainMenu entry ancestry exceeded bound")
    }
    #[test]
    fn sole_raw_sheet_router_leaves_actual_main_menu_settings_touch_reachable() {
        let (mut app,window,_)=scene_with_sheet(false);
        app.world_mut().resource_mut::<menu::ActiveMenu>().0=None;
        app.insert_resource(empty_play_world())
            .insert_resource(nvf_play::save::SaveConfig::with_dir(None))
            .init_resource::<nvf_play::pause::Paused>().init_resource::<nvf_play::pause::PlayClock>()
            .init_resource::<nvf_play::pause::Page>().init_resource::<nvf_play::pause::PageRequest>()
            .init_resource::<Assets<nvf_play::music::LayeredMusic>>()
            .add_message::<nvf_play::save::LoadRequest>().add_message::<nvf_play::intro::PlayIntro>()
            .add_message::<bevy::app::AppExit>().add_message::<nvf_play::ui_sounds::UiSound>()
            .insert_resource(nvf_play::main_menu::MainMenuStart).add_plugins(nvf_play::main_menu::MainMenuPlugin);
        for _ in 0..6 {app.update();}
        assert!(app.world().resource::<nvf_play::main_menu::MainMenu>().open);
        let label=fnv_util::words::tr("Settings").to_string();
        let (button,point)=actual_menu_button(&mut app,&label);
        {let mut stack=app.world_mut().resource_mut::<bevy::ui::UiStack>();stack.uinodes=vec![button];stack.partition.clear();stack.partition.push(0..1);}
        touch(&mut app,window,4116,TouchPhase::Started,point.x,point.y);app.update();
        assert!(app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4116));
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(),PointerState::Pressed);
        assert_eq!(app.world().resource::<nvf_play::pause::PageRequest>().0,Some(nvf_play::pause::Page::Settings {selected:None}),
            "real MainMenu choose receives the unowned touch and requests its existing Settings page");
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
        touch(&mut app,window,4116,TouchPhase::Ended,point.x,point.y);app.update();
    }

    #[derive(Resource, Default)]
    struct TouchedSetting(bool);

    #[test]
    fn overlay_press_over_real_settings_row_has_no_pointer_or_gameplay_effect() {
        use nvf_play::{pause::{self, Page, Paused}, settings, settings_view::RowId};
        let (mut app, window) = default_menu_touch_scene(nvf_play::pipboy::Page::Items, false);
        app.world_mut().resource_mut::<menu::ActiveMenu>().0 = None;
        app.world_mut().resource_mut::<nvf_play::pipboy::PipBoy>().phase = nvf_play::pipboy::Phase::Down;
        let camera = app.world().resource::<nvf_play::pipboy::PipBoyScreen>().camera;
        app.world_mut().entity_mut(camera).insert(bevy::ui::IsDefaultUiCamera);
        app.insert_resource(nvf_play::save::SaveConfig::with_dir(None))
            .init_resource::<nvf_play::pad::PadGlyphs>()
            .init_resource::<nvf_play::control::PlayerControl>()
            .init_resource::<bevy::ui::UiStack>()
            .init_resource::<TouchedSetting>()
            .add_message::<nvf_play::save::SaveRequest>().add_message::<nvf_play::save::SaveDone>()
            .add_message::<nvf_play::save::LoadRequest>().add_message::<nvf_play::ui_sounds::UiSound>()
            .add_plugins((pause::PausePlugin, ui_pointer::UiPointerPlugin))
            .add_systems(Update, nvf_play::control::read_controls)
            .add_systems(PostUpdate, (bevy::ui::update::update_clipping_system, ApplyDeferred, painted_ui_stack)
                .chain().after(bevy::ui::ui_layout_system));
        pause::use_touch_fixture(&mut app, "Touch fixture");
        settings::add_setting(&mut app, settings::Setting {
            key: "touch-fixture", group: "Touch fixture", label: "Touch fixture row", note: "", switches: "",
            get: |w| w.resource::<TouchedSetting>().0,
            set: |w, value| w.resource_mut::<TouchedSetting>().0 = value, slider: None,
        });
        let row = app.world().resource::<settings::Settings>().index("touch-fixture").unwrap();
        app.insert_resource(Paused(true)).insert_resource(Page::Settings { selected: Some(RowId::Setting(row)) });
        for _ in 0..4 { app.update(); }
        let (button, rect) = pause::fixture_row(app.world_mut(), RowId::Setting(row));
        let point = rect.center();
        let i = BUTTONS.iter().position(|b| b.key == Some(KeyCode::ArrowDown)).unwrap();
        let size = Vec2::splat(1000.0);
        let beside = point + Vec2::X * (rect.width() * 0.35);
        app.world_mut().resource_mut::<Layout>().config.buttons[i].center = point / size;

        touch(&mut app, window, 4117, TouchPhase::Started, point.x, point.y);
        app.update();
        assert!(matches!(app.world().resource::<TouchState>().contacts.get(&4117).map(|c| c.role),
            Some(Role::SettingsOverlay(index)) if index == i));
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4117));
        assert_ne!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed);
        assert_eq!(*app.world().resource::<Page>(), Page::Settings { selected: Some(RowId::Setting(row)) });
        assert!(!app.world().resource::<TouchedSetting>().0);
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        touch(&mut app, window, 4117, TouchPhase::Moved, beside.x, beside.y);
        app.update();
        touch(&mut app, window, 4117, TouchPhase::Ended, beside.x, beside.y);
        app.update();
        assert!(!app.world().resource::<TouchedSetting>().0, "a drag off the overlay cannot release onto the row");

        raw_tap(&mut app, window, 4118, beside);
        assert!(app.world().resource::<TouchedSetting>().0, "touch beside the overlay takes the painted row");
        app.world_mut().resource_mut::<Layout>().config.buttons[i].visible = false;
        raw_tap(&mut app, window, 4119, point);
        assert!(!app.world().resource::<TouchedSetting>().0, "hidden controls leave the row available");
    }

    #[test]
    fn sole_raw_sheet_router_distinct_id_budget_quarantines_unrecordable_ids_until_real_neutral() {
        let (mut app,window,_)=scene_with_sheet(false);let point=painted_footer(&mut app);
        for id in 4200..4235 {touch(&mut app,window,id,TouchPhase::Started,point.x,point.y);}
        app.update();
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4234),
            "ID overflow must mask even the contact that could not enter the fixed ID array");
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
        assert!(!app.world().resource::<nvf_play::pipboy::touch_items::TouchItems>().busy());
        for id in 4200..4235 {touch(&mut app,window,id,TouchPhase::Ended,point.x,point.y);}
        app.update();
        assert_eq!(app.world().resource::<Seen>().envelopes,0);
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4234));
        for _ in 0..6 {app.update();}
        assert!(app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(4234));
        let point=painted_footer(&mut app);raw_tap(&mut app,window,4236,point);
        assert_eq!(app.world().resource::<Seen>().envelopes,1,"ID flood cannot strand or re-arm the next genuine contact");
    }
    include!("menu_services_router_tests.rs");
}
