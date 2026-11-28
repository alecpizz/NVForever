// Normal-presentation CPU consumer fixtures; source-specific results are in the Android notes.
use crate::menu::normal_touch as normal;

fn normal_raw_owner(frame:Res<Fixture>,touches:Res<Touches>,mut events:MessageReader<TouchInput>,
    mut physical:MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut bridge:normal::Bridge,mut capture:ResMut<crate::ui_pointer::TouchUiCapture>) {
    capture.begin_frame();
    if physical.read().any(|e|e.window==frame.0.window){bridge.hardware_intent();}
    let neutral=touches.iter().next().is_none() && !touches.any_just_released() && !touches.any_just_canceled();
    bridge.prepare(Frame {neutral,..frame.0});
    for e in events.read(){bridge.event(e);}
    for t in touches.iter().chain(touches.iter_just_pressed()).chain(touches.iter_just_released())
        .chain(touches.iter_just_canceled()) {
        if bridge.captures_touch(frame.0.window,t.id()){capture.block(t.id());}
    }
    capture.suppress_pointer(bridge.visible());
}
type NormalRootNodes<'w,'s>=Query<'w,'s,(Entity,Option<&'static GlobalZIndex>),(With<Node>,Without<ChildOf>)>;

pub(crate) fn normal_ui_stack(mut stack:ResMut<bevy::ui::UiStack>,
    roots:NormalRootNodes<'_,'_>,
    children:Query<&Children>,nodes:Query<Entity,With<Node>>) {
    // Explicit CPU stack from the actual painter's root z order/child order,
    // after real layout. No authored ComputedNode, hit rect or PointerState.
    // This small known hierarchy is not a full renderer-stack qualification.
    fn walk(e:Entity,children:&Query<&Children>,nodes:&Query<Entity,With<Node>>,out:&mut Vec<Entity>) {
        if nodes.get(e).is_err(){return}
        out.push(e);
        if let Ok(kids)=children.get(e){for child in kids{walk(*child,children,nodes,out);}}
    }
    let mut roots:Vec<_>=roots.iter().map(|(e,z)|(z.map_or(0,|z|z.0),e)).collect();
    roots.sort_by_key(|(z,e)|(*z,e.to_bits()));
    stack.uinodes.clear();
    for (_,root) in roots{walk(root,&children,&nodes,&mut stack.uinodes);}
    stack.partition.clear();
    let n=stack.uinodes.len();if n>0 {stack.partition.push(0..n);}
}
fn normal_long_simulation() -> (crate::sim::SimState, FormId) {
    let mut misc = Vec::new();
    for (id, name, value) in std::iter::once((CAPS, "Coins".to_string(), 1i32))
        .chain((0..10).map(|i| (FormId(GOODS.0 + i), format!("Supply {i:02}: {}", "authored wrapped row words ".repeat(10)), 10))) {
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

fn normal_scene(native:bool)->(App,FormId) { normal_scene_with_sim(native,simulation()) }
fn normal_scene_with_sim(native:bool,pair:(crate::sim::SimState,FormId))->(App,FormId) {
    normal_scene_with_setup(native,pair,|_|{})
}
fn normal_scene_with_setup(native:bool,pair:(crate::sim::SimState,FormId),setup:impl FnOnce(&mut App))->(App,FormId) {
    use bevy::{app::{HierarchyPropagatePlugin,PropagateSet},
        camera::visibility::VisibilityPlugin,
        ui::{ComputedUiTargetCamera,ComputedUiRenderTargetInfo,UiStack,
            ui_surface::UiSurface,update::propagate_ui_target_cameras}};
    use crate::tiles::{barter_menu,container_menu,input,vanilla::TileMenus};
    let (mut sim,stock)=pair;
    let m=MenuModel::open_barter(&mut sim.world,stock,stock,CAPS,100.0,0).unwrap();
    let mut script=crate::script::ScriptState::over_no_plugins();
    crate::script::engine_menu_opened(Some(&mut script),m.mode);
    let mut tiles=barter_menu::BarterTiles::default();
    if native {
        let mut ui=barter_menu::tests::fixture(|s|s
            .replace("<rect name=\"BM_ItemsRect\"><x>60</x>","<rect name=\"BM_ItemsRect\"><x>60</x><y>200</y>")
            .replace("<rect name=\"BM_ContainerRect\"><x>460</x>","<rect name=\"BM_ContainerRect\"><x>460</x><y>200</y>")
            .replace("<image name=\"BM_ButtonX\">","<image name=\"BM_ButtonX\"><x>60</x><y>450</y><width>120</width><height>40</height>")
            .replace("<image name=\"BM_ButtonB\">","<image name=\"BM_ButtonB\"><x>460</x><y>450</y><width>120</width><height>40</height>"));
        let snap=barter_menu::Snapshot::of(&m,None,"Yours".into(),"Coins".into(),["A".into(),"X".into()]);
        barter_menu::tests::frame(&mut ui,&snap,&mut tiles);
        assert!(barter_menu::built(&ui).yes());
        let game=&mut script.rt.as_mut().unwrap().game;
        game.ui_tree=ui;game.ui_files=Some(Box::new(fnv_ui::MemSource::default()));
        game.menu=Some(crate::script::MENU_BARTER);
    }
    let mut app=App::new();
    app.add_plugins((MinimalPlugins,StatesPlugin,InputPlugin,bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin,bevy::text::TextPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_plugins((VisibilityPlugin,
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
            crate::ui_pointer::UiPointerPlugin))
        .init_resource::<UiSurface>().init_resource::<UiStack>()
        .init_resource::<crate::ui_pointer::TouchUiCapture>()
        .insert_state(crate::PlayState::Menu)
        .insert_resource(sim).insert_resource(script).insert_resource(ActiveMenu(Some(m)))
        .insert_resource(TileMenus {barter:native,..default()}).insert_resource(tiles)
        .insert_resource(Options {enabled:true})
        .init_resource::<UiScale>().init_resource::<crate::touch_safe_area::SafeInsets>()
        .init_resource::<crate::bindings::KeyboardCapture>().init_resource::<crate::free_cam::FreeCam>()
        .init_resource::<crate::pause::Paused>().init_resource::<crate::pad::PlatformInputGate>()
        .init_resource::<crate::bindings::Bindings>().init_resource::<crate::menu::PromptKeys>()
        .init_resource::<crate::menu::FreshMenu>().init_resource::<crate::menu::WheelHold>()
        .init_resource::<crate::pause::PlayClock>().init_resource::<crate::pad::PadGlyphs>()
        .init_resource::<Assets<Image>>()
        .init_resource::<input::MessageClick>().init_resource::<input::WaitSliderCheck>()
        .init_resource::<container_menu::ContainerTiles>().init_resource::<container_menu::TileWheelTitle>()
        .add_message::<crate::RefChanged>().add_message::<crate::ItemUsed>()
        .add_message::<crate::ui_sounds::UiSound>().add_message::<crate::crime::ItemsTaken>()
        .add_message::<crate::pickpocket::Pickpocketed>().add_message::<crate::wait::WaitInput>()
        .add_message::<crate::crafting::RecipeInput>().add_message::<crate::repair::RepairServicesInput>()
        .add_message::<crate::tutorial_menu::TutorialInput>().add_message::<CursorMoved>();
    let window=app.world_mut().spawn((Window {resolution:(640,360).into(),focused:true,..default()},PrimaryWindow)).id();
    let camera=app.world_mut().spawn((Camera2d,bevy::ui::IsDefaultUiCamera,Camera {
        computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {
            physical_size:UVec2::new(640,360),scale_factor:1.0}),..default()},..default()
    },RenderTarget::Window(WindowRef::Entity(window)))).id();
    app.insert_resource(Fixture(Frame {window,camera,size:Vec2::new(640.0,360.0),safe:default(),
        ui_scale:1.0,density:1.0,admitted:true,fresh:false,neutral:true}));
    install(&mut app);normal::install(&mut app);
    app.add_systems(PreUpdate,(normal_raw_owner.after(InputSystems).before(bevy::ui::UiSystems::Focus),
        bevy::ui::ui_focus_system.in_set(bevy::ui::UiSystems::Focus)));
    app.add_systems(Update,(input::feed,crate::menu::quantity_keys,crate::menu::menu_keys,
        crate::menu::menu_mouse,crate::menu::quantity_mouse,crate::menu::apply_input,
        crate::menu::draw).chain().in_set(crate::menu::MenuSystems));
    app.add_systems(PostUpdate,(ApplyDeferred,propagate_ui_target_cameras,
        bevy::ui::sync_font_size_to_em_size,bevy::ui::widget::measure_text_system,bevy::ui::ui_layout_system,
        bevy::ui::update::update_clipping_system,ApplyDeferred,normal_ui_stack).chain()
        .after(bevy::text::load_font_assets_into_font_collection)
        .after(bevy::text::detect_text_needs_rerender));
    app.configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default()
        .after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
    app.configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default()
        .after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
    if native {barter_menu::tests::install_normal_writer(&mut app);}
    setup(&mut app);
    for _ in 0..5{app.update();}
    let fallback=app.world_mut().query_filtered::<Entity,With<crate::menu::RowButton>>().iter(app.world()).count();
    assert_eq!(fallback==0,native,"real ordinary draw selected native versus handbuilt presentation");
    assert!(!app.world().resource::<TouchBarter>().visible(),"the historical replacement sheet is never prepared");
    (app,stock)
}
fn normal_point(app:&mut App,wanted:impl Fn(normal::Target)->bool)->Vec2 {
    let f=app.world().resource::<Fixture>().0;
    let ui=&app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.ui_tree;
    let native=app.world().resource::<crate::tiles::barter_menu::BarterTiles>();
    let tiled_qty = model(app).quantity.is_some() && crate::tiles::menus::normal_touch_quantity_shown(ui);
    if tiled_qty || (native.menu().is_some() && model(app).quantity.is_none()) {
        let menu=if tiled_qty {ui.menu(crate::tiles::menus::QUANTITY_MENU).unwrap()}else{native.menu().unwrap()};
        for c in fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure) {
            let r=c.region();if r.width<=0.0 || r.height<=0.0{continue}
            let p=crate::interface::to_menu_frame(Vec2::new((r.left+r.width/2.0) as f32,
                (r.top+r.height/2.0) as f32)*(f.size.y/960.0),f.size,Vec2::splat(0.5));
            let hit = if tiled_qty {
                crate::tiles::menus::normal_touch_quantity_hit(ui, p, f.size)
            } else {
                native.normal_touch_hit(ui, model(app), p, f.size)
            };
            if hit.is_some_and(|h|wanted(h.target)){return p}
        }
    } else {
        let world=app.world_mut();
        let mut q=world.query::<(&ComputedNode,&bevy::ui::UiGlobalTransform,
            Option<&crate::menu::RowButton>,Option<&crate::menu::CmdButton>,Option<&crate::menu::AmountMeter>,
            Option<&crate::menu::CategoryTitle>,Option<&bevy::ui::CalculatedClip>)>();
        for (node,t,row,command,meter,cat_title,clip) in q.iter(world) {
            if node.size().min_element()<=0.0{continue}
            let target=if let Some(r)=row{normal::Target::Row(r.0,r.1)}
                else if let Some(c)=command{normal::Target::Command(c.0)}
                else if meter.is_some(){normal::Target::Meter(0.5)}
                else if let Some(ct)=cat_title{normal::Target::CategoryHeader(ct.0)}else{continue};
            if wanted(target) {
                let mut rect=Rect::from_center_size(t.translation,node.size());
                if let Some(clip)=clip {
                    let Some(rects)=clip.rects() else{continue};
                    for c in rects {
                        // This fixture's ordinary painter uses axis-aligned UI.
                        // Derive its visible area from the real clipping output.
                        let to_world=c.world_to_clip_local.inverse();
                        let a=to_world.transform_point2(c.rect.min);
                        let b=to_world.transform_point2(c.rect.max);
                        rect=rect.intersect(Rect::from_corners(a.min(b),a.max(b)));
                    }
                }
                rect=rect.intersect(Rect::from_corners(Vec2::ZERO,f.size*f.density));
                if rect.width()>0.0 && rect.height()>0.0 {return rect.center()/f.density}
            }
        }
    }
    panic!("actual painted target missing");
}
fn normal_tap_at(app:&mut App,p:Vec2) {
    edge(app,201,TouchPhase::Started,p);app.update();
    edge(app,201,TouchPhase::Ended,p);app.update();app.update();
}
fn normal_command(app:&mut App,command:Command) {
    let p=normal_point(app,|t|t==normal::Target::Command(command));normal_tap_at(app,p);
}
fn normal_scroll_value(app:&App,native:bool,side:Side)->f64 {
    if native {app.world().resource::<crate::tiles::barter_menu::BarterTiles>().normal_touch_value(side)}
    else {model(app).scroll[side as usize] as f64}
}
#[derive(Resource,Default)]
struct NormalLateCaps(Option<FormId>);
fn normal_change_caps(mut change:ResMut<NormalLateCaps>,mut sim:ResMut<crate::sim::SimState>) {
    if let Some(stock)=change.0.take(){sim.world.inventory_mut(stock).unwrap().remove(CAPS,999);}
}

#[test]
fn normal_barter_both_painted_lists_drag_without_select_stage_or_caps_change() {
    for native in [false,true] {
        let (mut app,stock)=normal_scene(native);
        let selected=model(&app).selected;let focus=model(&app).focus;
        for side in [Side::Player,Side::Container] {
            let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(s,_) if s==side));
            let before=normal_scroll_value(&app,native,side);
            edge(&mut app,202,TouchPhase::Started,p);app.update();
            assert_eq!(model(&app).selected,selected,"Started must not select");
            edge(&mut app,202,TouchPhase::Moved,p-Vec2::Y*32.0);app.update();
            edge(&mut app,202,TouchPhase::Ended,p-Vec2::Y*32.0);app.update();
            assert!(normal_scroll_value(&app,native,side)>before,"actual painted view must move");
            assert_eq!(model(&app).selected,selected);
            assert_eq!(model(&app).focus,focus);
            assert_eq!(offered(&app),0);
            assert!(model(&app).quantity.is_none());
            app.update();
        }
        assert_eq!(count(&mut app,PLAYER,CAPS),1000);
        assert_eq!(count(&mut app,stock,CAPS),1000);
    }
}

#[test]
fn normal_barter_real_row_end_quantity_confirm_and_accept_use_existing_effects_once() {
    for native in [false,true] {
        let (mut app,stock)=normal_scene(native);
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        edge(&mut app,203,TouchPhase::Started,p);app.update();
        assert!(model(&app).quantity.is_none());
        assert_eq!(offered(&app),0);
        edge(&mut app,203,TouchPhase::Ended,p);app.update();
        if native {
            let game=&app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game;
            assert!(game.ui_pointer.active(&game.ui_tree).is_some(),"the actual native feed handled this End");
            assert!(game.ui_pointer.pending(),"the native clicked pulse was preserved, not replaced by a direct effect");
        }
        app.update();
        assert_eq!(model(&app).quantity.unwrap().max,8,"ordinary row command opens real quantity");
        let picked=model(&app).quantity.unwrap();
        let item=model(&app).list(picked.side).rows[picked.row].item;
        assert_eq!(model(&app).quantity.unwrap().amount,8,"normal prompt retains its original maximum default");
        normal_command(&mut app,Command::AmountStep(-1));
        assert_eq!(model(&app).quantity.unwrap().amount,7);
        normal_command(&mut app,Command::AmountOk);
        assert_eq!(offered(&app),7);
        assert_eq!(count(&mut app,PLAYER,item),8,"staging is not accepting the trade");
        normal_command(&mut app,Command::Accept);
        assert_eq!(offered(&app),0);
        assert_eq!(count(&mut app,PLAYER,item),15);
        assert_eq!(count(&mut app,stock,item),1);
        let caps=count(&mut app,PLAYER,CAPS);
        app.update();app.update();
        assert_eq!(count(&mut app,PLAYER,CAPS),caps,"terminal frames cannot replay acceptance");
    }
}

#[test]
fn normal_barter_cancel_secondary_finger_and_changed_stock_cannot_commit_a_held_tap() {
    for native in [false,true] {
        let (mut app,stock)=normal_scene(native);
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        edge(&mut app,204,TouchPhase::Started,p);app.update();
        edge(&mut app,205,TouchPhase::Started,p);app.update();
        edge(&mut app,205,TouchPhase::Ended,p);app.update();
        assert_eq!(offered(&app),0);
        assert!(model(&app).quantity.is_none(),"another finger cannot take the owning contact");
        edge(&mut app,204,TouchPhase::Canceled,p);app.update();app.update();
        assert_eq!(offered(&app),0);
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        edge(&mut app,206,TouchPhase::Started,p);app.update();
        let index=model(&app).selected[Side::Container as usize];
        let item=model(&app).list(Side::Container).rows[index].item;
        app.world_mut().resource_mut::<crate::sim::SimState>().world.inventory_mut(stock).unwrap().remove(item,8);
        edge(&mut app,206,TouchPhase::Ended,p);app.update();
        assert_eq!(offered(&app),0);
        assert!(model(&app).quantity.is_none(),"late real inventory change is rejected at consumption");
        assert_eq!(count(&mut app,PLAYER,item),8);
        app.update();app.update();
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        normal_tap_at(&mut app,p);
        assert!(model(&app).quantity.is_some(),"a new stable real row tap remains usable after refusal");
        normal_command(&mut app,Command::AmountCancel);
        assert!(model(&app).quantity.is_none(),"ordinary quantity Cancel remains the effect authority");
        assert_eq!(offered(&app),0);
    }
}

#[test]
fn normal_barter_actual_late_quote_change_and_duplicate_end_do_not_spend_caps_or_items() {
    for native in [false,true] {
        let (mut app,stock)=normal_scene(native);
        app.init_resource::<NormalLateCaps>()
            .add_systems(Update,normal_change_caps.after(normal::Dispatch).before(crate::menu::MenuSystems));
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Player,_)));
        normal_tap_at(&mut app,p);
        for _ in 0..7{normal_command(&mut app,Command::AmountStep(-1));}
        assert_eq!(model(&app).quantity.unwrap().amount,1);
        normal_command(&mut app,Command::AmountOk);
        assert_eq!(offered(&app),1);
        let p=normal_point(&mut app,|t|t==normal::Target::Command(Command::Accept));
        edge(&mut app,207,TouchPhase::Started,p);app.update();
        app.world_mut().resource_mut::<NormalLateCaps>().0=Some(stock);
        edge(&mut app,207,TouchPhase::Ended,p);app.update();
        assert_eq!(offered(&app),1,"real consumer refuses changed purse after dispatch");
        assert_eq!(count(&mut app,PLAYER,CAPS),1000);
        assert_eq!(count(&mut app,stock,CAPS),1,"only the explicit fixture world mutation occurred");
        assert_eq!(count(&mut app,PLAYER,GOODS),8);
        assert_eq!(count(&mut app,stock,GOODS),8);
        edge(&mut app,207,TouchPhase::Ended,p);app.update();app.update();
        assert_eq!(offered(&app),1,"a stale terminal event cannot re-arm the consumed token");
        assert_eq!(count(&mut app,PLAYER,CAPS),1000);
    }
}

#[test]
fn normal_barter_actual_mouse_takeover_keeps_hardware_and_consumes_first_touch_return() {
    use bevy::input::{ButtonState,mouse::MouseButtonInput};
    for native in [false,true] {
        let (mut app,_)=normal_scene(native);
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        let window=app.world().resource::<Fixture>().0.window;
        edge(&mut app,208,TouchPhase::Started,p);app.update();
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::new(630.0,350.0)));
        app.world_mut().write_message(MouseButtonInput {window,button:MouseButton::Left,state:ButtonState::Pressed});
        app.update();
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        edge(&mut app,208,TouchPhase::Ended,p);app.update();
        assert!(model(&app).quantity.is_none());
        assert_eq!(offered(&app),0);
        app.world_mut().write_message(MouseButtonInput {window,button:MouseButton::Left,state:ButtonState::Released});
        app.update();
        normal_tap_at(&mut app,p);
        assert!(model(&app).quantity.is_none(),"first touch only returns from actual hardware provenance");
        normal_tap_at(&mut app,p);
        assert!(model(&app).quantity.is_some(),"a later fresh touch uses the normal painted row");
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }
}

#[test]
fn normal_barter_contact_budget_discards_proposals_and_real_neutral_recovers() {
    for native in [false,true] {
        let (mut app,_)=normal_scene(native);
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        for id in 300..335{edge(&mut app,id,TouchPhase::Started,p);}
        app.update();
        assert!(model(&app).quantity.is_none());
        assert_eq!(offered(&app),0);
        assert!(!app.world().resource::<crate::ui_pointer::TouchUiCapture>().allows(334),
            "overflow masks even the ID beyond bounded retirement storage");
        for id in 300..335{edge(&mut app,id,TouchPhase::Ended,p);}
        app.update();
        assert!(model(&app).quantity.is_none(),"overflow cannot commit its former owner on End");
        app.update();app.update();
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        normal_tap_at(&mut app,p);
        assert!(model(&app).quantity.is_some(),"real neutral releases the bounded quarantine");
    }
}

#[test]
fn normal_barter_registered_off_on_does_not_revive_a_held_proposal() {
    for native in [false,true] {
        let (mut app,_)=normal_scene(native);
        app.insert_resource(crate::settings::SettingsFile(None));
        let row=app.world().resource::<crate::settings::Settings>().index(KEY).unwrap();
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        edge(&mut app,210,TouchPhase::Started,p);app.update();
        assert_eq!(crate::settings::flip(app.world_mut(),row),Some(false));
        assert_eq!(crate::settings::flip(app.world_mut(),row),Some(true));
        edge(&mut app,210,TouchPhase::Ended,p);app.update();
        assert!(model(&app).quantity.is_none(),"actual registered setter canceled the old epoch");
        assert_eq!(offered(&app),0);
        app.update();app.update();
        let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
        normal_tap_at(&mut app,p);
        assert!(model(&app).quantity.is_some());
        assert!(app.world().resource::<Options>().enabled,"persisted toggle identity and enabled state retained");
    }
}

#[test]
fn normal_barter_wrapped_handbuilt_row_controls_actual_drag_quantum_not_fixed_pixels() {
    let (mut app,_)=normal_scene_with_sim(false,normal_long_simulation());
    let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Player,0)));
    let density=app.world().resource::<Fixture>().0.density;
    let world=app.world_mut();
    let mut q=world.query::<(&ComputedNode,&crate::menu::RowButton)>();
    let height=q.iter(world).find_map(|(node,row)|
        (row.0==Side::Player && row.1==0).then_some(node.size().y/density)).unwrap();
    assert!(height>40.0,"authored FULL text must actually wrap in the ordinary painter");
    let selected=model(&app).selected;
    edge(&mut app,211,TouchPhase::Started,p);app.update();
    edge(&mut app,211,TouchPhase::Moved,p-Vec2::Y*(height*0.6));app.update();
    assert_eq!(model(&app).scroll[Side::Player as usize],0,
        "less than one actual painted row accumulates without a premature fixed-20px step");
    edge(&mut app,211,TouchPhase::Moved,p-Vec2::Y*(height*1.2));app.update();
    assert_eq!(model(&app).scroll[Side::Player as usize],1);
    assert_eq!(model(&app).selected,selected);
    edge(&mut app,211,TouchPhase::Ended,p-Vec2::Y*(height*1.2));app.update();
    assert!(model(&app).quantity.is_none());
    assert_eq!(offered(&app),0);
}

#[test]
fn normal_barter_native_same_row_replacement_stock_retires_held_quote_then_fresh_tap_works() {
    let (mut app,merchant)=normal_scene(true);
    let side=Side::Container;
    let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(s,0) if s==side));
    let old_menu=app.world().resource::<crate::tiles::barter_menu::BarterTiles>().menu();
    let row=&model(&app).list(side).rows[0];
    let original=(crate::menu::model::RowKey::at(&model(&app).list(side).rows,0).unwrap(),row.count,row.price);
    edge(&mut app,213,TouchPhase::Started,p);app.update();
    // Real ordinary constructor over a second equally stocked vendor container:
    // the merchant, displayed row and price remain equal; only the trade changes.
    let (replacement,next)={
        let mut sim=app.world_mut().resource_mut::<crate::sim::SimState>();
        let replacement=sim.world.place(BOX,None,[0.0;3],[0.0;3],1).unwrap();
        sim.world.add_item(replacement,CAPS,1000).unwrap();
        for i in 0..10 {sim.world.add_item(replacement,FormId(GOODS.0+i),8).unwrap();}
        let next=MenuModel::open_barter(&mut sim.world,merchant,replacement,CAPS,100.0,0).unwrap();
        (replacement,next)
    };
    app.world_mut().resource_mut::<ActiveMenu>().0=Some(next);
    app.update();
    let row=&model(&app).list(side).rows[0];
    assert_eq!((crate::menu::model::RowKey::at(&model(&app).list(side).rows,0).unwrap(),row.count,row.price),original);
    assert_eq!(app.world().resource::<crate::tiles::barter_menu::BarterTiles>().menu(),old_menu,
        "the same real native menu remains selected and its rows are repainted");
    assert_eq!(normal_point(&mut app,|t|matches!(t,normal::Target::Row(s,0) if s==side)),p);
    edge(&mut app,213,TouchPhase::Ended,p);app.update();app.update();
    assert!(model(&app).quantity.is_none(),"equal row identity cannot approve an older stock/quote");
    assert_eq!(offered(&app),0);
    assert_eq!(count(&mut app,merchant,GOODS),8);
    assert_eq!(count(&mut app,replacement,GOODS),8);
    let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(s,0) if s==side));
    normal_tap_at(&mut app,p);
    assert!(model(&app).quantity.is_some(),"a fresh contact uses the new ordinary trade");
    assert_eq!(model(&app).trade.as_ref().unwrap().stock,replacement);
    normal_command(&mut app,Command::AmountCancel);
    assert!(model(&app).quantity.is_none());
    assert_eq!(offered(&app),0);
}

#[test]
fn handbuilt_barter_with_tiled_quantity_menu_supports_full_touch_without_softlock() {
    // Reproduces the exact Android default: hand-built BarterMenu (TileMenus.barter == false)
    // with tiled QuantityMenu (TileMenus.quantity == true).
    let (mut app, stock) = normal_scene(false);
    let q_xml = r#"<menu name="QuantityMenu"><rect name="NOGLOW_BRANCH"><image name="QM_Background"><x>0</x><y>0</y><width>1280</width><height>960</height></image><rect name="QM_MainRect"><x>270</x><y>295</y><width>740</width><height>370</height><rect name="QM_AmountMeter"><x>149</x><y>115</y><width>391</width><height>24</height><target>1</target><user0>1</user0><user2>1</user2><user3>1</user3><user4>5</user4></rect><image name="QM_DecreaseArrow"><x>130</x><y>110</y><width>18</width><height>35</height><target>1</target></image><image name="QM_IncreaseArrow"><x>549</x><y>110</y><width>18</width><height>35</height><target>1</target></image><image name="QM_OkButton"><x>580</x><y>155</y><width>85</width><height>35</height><target>1</target></image><image name="QM_CancelButton"><x>580</x><y>200</y><width>85</width><height>35</height><target>1</target></image></rect></rect></menu>"#;
    {
        let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
        let game = &mut script.rt.as_mut().unwrap().game;
        let src = fnv_ui::MemSource::default().with("menus\\quantity_menu.xml", q_xml);
        game.ui_tree.load_menu(&src, "menus\\quantity_menu.xml").unwrap();
        game.ui_files = Some(Box::new(src));
        game.ui_tree.update();
    }
    app.world_mut().resource_mut::<crate::tiles::vanilla::TileMenus>().quantity = true;
    app.update();
    // Open quantity prompt by tapping the 8-stack item row.
    let p = normal_point(&mut app, |t| matches!(t, normal::Target::Row(Side::Container, _)));
    normal_tap_at(&mut app, p);
    assert!(model(&app).quantity.is_some());
    assert_eq!(model(&app).quantity.unwrap().amount, 8);
    // Tap decrease arrow (-)
    let dec = normal_point(&mut app, |t| t == normal::Target::Command(Command::AmountStep(-1)));
    normal_tap_at(&mut app, dec);
    assert_eq!(model(&app).quantity.unwrap().amount, 7);
    // Tap increase arrow (+)
    let inc = normal_point(&mut app, |t| t == normal::Target::Command(Command::AmountStep(1)));
    normal_tap_at(&mut app, inc);
    assert_eq!(model(&app).quantity.unwrap().amount, 8);
    // Tap OK to confirm
    let ok = normal_point(&mut app, |t| t == normal::Target::Command(Command::AmountOk));
    normal_tap_at(&mut app, ok);
    assert!(model(&app).quantity.is_none());
    assert_eq!(offered(&app), 8);
    // Open again on player's 8-stack and tap outside QM_MainRect (backdrop cancel)
    let p2 = normal_point(&mut app, |t| matches!(t, normal::Target::Row(Side::Player, _)));
    normal_tap_at(&mut app, p2);
    assert!(model(&app).quantity.is_some());
    normal_tap_at(&mut app, Vec2::new(10.0, 10.0));
    assert!(model(&app).quantity.is_none(), "tapping outside tiled QuantityMenu cancels without soft-lock");
    assert_eq!(count(&mut app, stock, GOODS), 8);
}

#[test]
fn container_quantity_menu_supports_touch_adjust_confirm_and_cancel() {
    let (mut app, stock) = normal_scene(false);
    let next = {
        let mut sim = app.world_mut().resource_mut::<crate::sim::SimState>();
        MenuModel::open_container(&mut sim.world, stock).unwrap()
    };
    let index = next.list(Side::Container).rows.iter().position(|r| r.item == GOODS).unwrap();
    crate::script::engine_menu_opened(Some(&mut app.world_mut().resource_mut::<crate::script::ScriptState>()), next.mode);
    app.world_mut().resource_mut::<ActiveMenu>().0 = Some(next);
    app.world_mut().write_message(MenuInput(Command::Hover(Side::Container, index)));
    app.world_mut().write_message(MenuInput(Command::One));
    for _ in 0..3 { app.update(); }
    assert!(matches!(model(&app).mode, Mode::Container(_)));
    assert_eq!(model(&app).quantity.unwrap().amount, 8);
    let dec = normal_point(&mut app, |t| t == normal::Target::Command(Command::AmountStep(-1)));
    normal_tap_at(&mut app, dec);
    assert_eq!(model(&app).quantity.unwrap().amount, 7);
    let ok = normal_point(&mut app, |t| t == normal::Target::Command(Command::AmountOk));
    normal_tap_at(&mut app, ok);
    assert!(model(&app).quantity.is_none());
    assert_eq!(count(&mut app, PLAYER, GOODS), 15);
    assert_eq!(count(&mut app, stock, GOODS), 1);
}

#[test]
fn barter_category_header_horizontal_swipe_switches_categories() {
    let (mut app, _) = normal_scene(false);
    let p = normal_point(&mut app, |t| t == normal::Target::CategoryHeader(Side::Player));
    assert_eq!(model(&app).category[Side::Player as usize], Category::All);
    // Swipe right -> next category (+1)
    edge(&mut app, 310, TouchPhase::Started, p); app.update();
    edge(&mut app, 310, TouchPhase::Moved, p + Vec2::new(40.0, 0.0)); app.update();
    edge(&mut app, 310, TouchPhase::Ended, p + Vec2::new(45.0, 0.0)); app.update(); app.update();
    let after_right = model(&app).category[Side::Player as usize];
    assert_ne!(after_right, Category::All, "right swipe on Items header advances category");
    // Swipe left -> back (-1) to All
    let p2 = normal_point(&mut app, |t| t == normal::Target::CategoryHeader(Side::Player));
    edge(&mut app, 311, TouchPhase::Started, p2); app.update();
    edge(&mut app, 311, TouchPhase::Moved, p2 - Vec2::new(40.0, 0.0)); app.update();
    edge(&mut app, 311, TouchPhase::Ended, p2 - Vec2::new(45.0, 0.0)); app.update(); app.update();
    assert_eq!(model(&app).category[Side::Player as usize], Category::All, "left swipe on Items header reverses category");
}
