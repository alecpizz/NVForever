// Existing normal fixture/sole core raw owner and real MenuInput/apply_input.
// No PointerState write, synthetic physical state or direct quantity assignment.
fn meter_load_native(ui:&mut fnv_ui::Ui) {
        crate::tiles::menus::load_quantity_fixture_for_touch_tests(ui,|xml|xml
            .replace("<rect name=\"QM_MainRect\">","<rect name=\"QM_MainRect\"><x>120</x><y>250</y><width>700</width><height>210</height>")
            .replace("<rect name=\"QM_AmountMeter\"/>",r#"<rect name="QM_AmountMeter"><x>80</x><y>70</y><width>480</width><height>48</height><target>1</target><hotrect name="QM_MeterHit"><width>480</width><height>48</height><target>1</target></hotrect><text name="QM_AmountText"><string><copy src="parent()" trait="user2"/></string><width>80</width><height>25</height></text></rect><image name="QM_OKButton"><x>570</x><y>60</y><width>100</width><height>40</height><target>1</target><text name="button_text"><string>OK</string><width>100</width><height>30</height></text></image><image name="QM_CancelButton"><x>570</x><y>120</y><width>100</width><height>40</height><target>1</target><text name="button_text"><string>Cancel</string><width>100</width><height>30</height></text></image>"#));
        assert!(crate::tiles::menus::quantity_built(ui).yes());
}
fn meter_scene(native:bool,most:i32)->(App,FormId) {
    let (mut sim,stock)=simulation();
    if most>8 {sim.world.add_item(stock,GOODS,most-8).unwrap();}
    let (mut app,stock)=normal_scene_with_sim(native,(sim,stock));
    app.insert_resource(crate::settings::SettingsFile(None));
    if native {
        let mut script=app.world_mut().resource_mut::<crate::script::ScriptState>();
        let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;
        meter_load_native(ui);
    }
    if native {crate::tiles::install_quantity_writer_for_touch_tests(&mut app);}
    let index=model(&app).list(Side::Container).rows.iter().position(|r|r.item==GOODS).unwrap();
    let row=normal_point(&mut app,|t|t==normal::Target::Row(Side::Container,index));
    normal_tap_at(&mut app,row);for _ in 0..4 {app.update();}
    assert_eq!(model(&app).quantity.unwrap().max,most,"actual ordinary row opened the prompt");
    assert_eq!(model(&app).quantity.unwrap().amount,most);
    let is_native={let game=&app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game;
        app.world().resource::<crate::tiles::barter_menu::BarterTiles>().normal_touch_quantity(&game.ui_tree)};
    assert_eq!(is_native,native);
    let nodes=app.world_mut().query_filtered::<Entity,With<crate::menu::AmountMeter>>().iter(app.world()).count();
    assert_eq!(nodes==0,native,"actual normal draw chose native or handbuilt prompt");
    (app,stock)
}
fn meter_display(app:&mut App,density:f32,scale:f32) {
    let f=app.world().resource::<Fixture>().0;
    let physical=(f.size*density).as_uvec2();let mut resolution=bevy::window::WindowResolution::new(physical.x,physical.y);
    resolution.set_scale_factor_override(Some(density));app.world_mut().get_mut::<Window>(f.window).unwrap().resolution=resolution;
    app.world_mut().get_mut::<Camera>(f.camera).unwrap().computed.target_info=Some(RenderTargetInfo {physical_size:physical,scale_factor:density});
    app.insert_resource(UiScale(scale));
    {let mut fixture=app.world_mut().resource_mut::<Fixture>();fixture.0.density=density;fixture.0.ui_scale=scale;}
    for _ in 0..4 {app.update();}
    assert_eq!(app.world().get::<Window>(f.window).unwrap().scale_factor(),density);
}
// Observe deliberately invalidated paint without asserting it is an input target.
fn meter_geometry(app:&mut App,native:bool)->Rect {
    let f=app.world().resource::<Fixture>().0;
    let bounds=if native {
        let game=&app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game;
        let b=crate::tiles::menus::meter_box(&game.ui_tree).unwrap();
        let to_window=|x:f64,y:f64|crate::interface::to_menu_frame(Vec2::new(x as f32,y as f32)*(f.size.y/960.0),f.size,Vec2::splat(0.5));
        Rect::from_corners(to_window(b.left,b.top),to_window(b.left+b.width,b.top+b.height))
    } else {
        let world=app.world_mut();let mut q=world.query_filtered::<(&ComputedNode,&bevy::ui::UiGlobalTransform),With<crate::menu::AmountMeter>>();
        let (node,at)=q.single(world).unwrap();assert!(node.size().min_element()>0.0);
        Rect::from_center_size(at.translation/f.density,node.size()/f.density)
    };
    assert!(bounds.min.is_finite() && bounds.max.is_finite() && bounds.width()>0.0 && bounds.height()>0.0,
        "invalid actual meter bounds: native={native} density={} ui_scale={} bounds={bounds:?}",f.density,f.ui_scale);
    bounds
}
fn meter_bounds(app:&mut App,native:bool)->Rect {
    let f=app.world().resource::<Fixture>().0;
    let bounds=meter_geometry(app,native);
    let viewport=Rect::from_corners(Vec2::ZERO,f.size);
    // Witness each live sample region through the actual consumer, not merely
    // through a ComputedNode or XML rectangle that might not be interactive.
    for point in [meter_at(bounds,0.25),meter_at(bounds,0.5),meter_at(bounds,0.75),
        Vec2::new(bounds.max.x-2.0,bounds.center().y)] {
        let phase=TouchPhase::Started;
        assert!(viewport.contains(point),
            "meter sample outside actual viewport: native={native} density={} ui_scale={} phase={phase:?} point={point:?} bounds={bounds:?} viewport={viewport:?}",f.density,f.ui_scale);
        let hit=normal::meter_fixture_hit(app.world_mut(),f,point);
        assert!(hit.is_some_and(|h|matches!(h.target,normal::Target::Meter(_))
            && matches!((native,h.source),(true,normal::Source::Tile(..))|(false,normal::Source::Node(_)))),
            "actual top hit is not this meter: native={native} density={} ui_scale={} phase={phase:?} point={point:?} bounds={bounds:?} hit={hit:?}",f.density,f.ui_scale);
    }
    bounds
}
fn meter_at(rect:Rect,part:f32)->Vec2 {Vec2::new(rect.min.x+rect.width()*part,rect.center().y)}
fn meter_paint(app:&mut App,native:bool)->f32 {
    // The real native writer precedes UI input, so its following ordinary frame
    // paints a just-applied amount. No test painter or quantity effect is injected.
    app.update();
    if native {
        let ui=&app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.ui_tree;
        let amount=ui.get_float("QuantityMenu/NOGLOW_BRANCH/QM_MainRect/QM_AmountMeter/user2").unwrap();
        let text=ui.find_tile("QuantityMenu/NOGLOW_BRANCH/QM_MainRect/QM_AmountMeter/QM_AmountText").unwrap();
        assert_eq!(ui.tile(text).get("string").unwrap().text(),format!("{}",model(app).quantity.unwrap().amount));
        assert_eq!(amount, f64::from(model(app).quantity.unwrap().amount));amount as f32
    } else {
        let bounds=meter_bounds(app,false);let density=app.world().resource::<Fixture>().0.density;
        let world=app.world_mut();let mut meters=world.query_filtered::<&ChildOf,With<crate::menu::AmountMeter>>();
        let parent=meters.single(world).unwrap().parent();
        let mut area=0.0;
        for child in world.get::<Children>(parent).unwrap() {
            if !world.get::<BackgroundColor>(*child).is_some_and(|c|c.0.to_linear().alpha>0.99){continue}
            let Some((node,at))=world.get::<ComputedNode>(*child).zip(world.get::<bevy::ui::UiGlobalTransform>(*child)) else {continue};
            let r=Rect::from_center_size(at.translation/density,node.size()/density);let overlap=r.intersect(bounds);
            if r.width()<bounds.width()*0.1 && overlap.width()>0.0 && overlap.height()>0.0 {area+=overlap.width()*overlap.height();}
        }
        assert!(area>0.0,"actual fallback meter tick nodes are visible, not hidden Text metadata");area
    }
}
fn meter_hardware(app:&mut App,p:Vec2,pressed:bool) {
    use bevy::input::{ButtonState,mouse::MouseButtonInput};
    let w=app.world().resource::<Fixture>().0.window;
    app.world_mut().get_mut::<Window>(w).unwrap().set_cursor_position(Some(p));
    app.world_mut().write_message(MouseButtonInput {window:w,button:MouseButton::Left,state:if pressed {ButtonState::Pressed}else{ButtonState::Released}});
    app.update();
}
#[test]
fn ordinary_quantity_meter_real_mouse_drag_still_changes_amount_without_confirming() {
    for native in [false,true] {
        let (mut app,stock)=meter_scene(native,8);let bounds=meter_bounds(&mut app,native);
        meter_hardware(&mut app,meter_at(bounds,0.25),true);
        assert_eq!(model(&app).quantity.unwrap().amount,2,"unchanged physical baseline positive");
        let p=meter_at(bounds,0.75);let w=app.world().resource::<Fixture>().0.window;
        app.world_mut().get_mut::<Window>(w).unwrap().set_cursor_position(Some(p));app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,6);
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        meter_hardware(&mut app,p,false);
        assert_eq!(offered(&app),0);assert_eq!(count(&mut app,PLAYER,CAPS),1000);assert_eq!(count(&mut app,stock,GOODS),8);
    }
}
#[test]
fn ordinary_quantity_meter_touch_updates_live_across_real_redraw_then_clamps_without_confirming() {
    for (native,density,scale) in [(false,1.,1.),(false,2.,1.5),(true,1.,1.),(true,2.,0.75)] {
        let (mut app,stock)=meter_scene(native,8);meter_display(&mut app,density,scale);let bounds=meter_bounds(&mut app,native);let p=meter_at(bounds,0.25);
        edge(&mut app,501,TouchPhase::Started,p);app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,2);
        let low=meter_paint(&mut app,native);
        edge(&mut app,501,TouchPhase::Moved,meter_at(bounds,0.26));app.update();app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,2,"same quantized amount must not strand the held owner");
        edge(&mut app,501,TouchPhase::Moved,meter_at(bounds,0.75));app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,6,"same held ID survived its own actual amount repaint");
        assert!(meter_paint(&mut app,native)>low,"actual native text trait or handbuilt fill geometry follows while held");
        edge(&mut app,501,TouchPhase::Moved,meter_at(bounds,-0.2)+Vec2::Y*24.);app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,1,"owned movement outside the meter clamps, without clicking nearby UI");
        edge(&mut app,501,TouchPhase::Ended,meter_at(bounds,-0.2)+Vec2::Y*24.);app.update();app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,1);assert_eq!(offered(&app),0);
        assert_eq!(count(&mut app,PLAYER,CAPS),1000);assert_eq!(count(&mut app,stock,GOODS),8);
        normal_command(&mut app,Command::AmountOk);assert_eq!(offered(&app),1,"only a separate real Confirm stages units");
        assert_eq!(count(&mut app,PLAYER,GOODS),8,"confirmation is still separate from trade acceptance");
    }
}
#[test]
fn ordinary_quantity_meter_cancel_and_secondary_cannot_confirm_or_apply_cancel_position() {
    for native in [false,true] {
        let (mut app,_)=meter_scene(native,8);let bounds=meter_bounds(&mut app,native);let p=meter_at(bounds,0.25);
        edge(&mut app,502,TouchPhase::Started,p);edge(&mut app,502,TouchPhase::Moved,meter_at(bounds,0.75));
        edge(&mut app,502,TouchPhase::Canceled,meter_at(bounds,0.0));app.update();app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,8,"same-batch cancel discards pending live proposal");
        edge(&mut app,503,TouchPhase::Started,p);app.update();assert_eq!(model(&app).quantity.unwrap().amount,2);
        let confirm=normal_point(&mut app,|t|t==normal::Target::Command(Command::AmountOk));
        edge(&mut app,504,TouchPhase::Started,confirm);app.update();edge(&mut app,504,TouchPhase::Ended,confirm);app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,2);assert_eq!(offered(&app),0,"second finger cannot confirm owner's prompt");
        edge(&mut app,503,TouchPhase::Moved,meter_at(bounds,0.75));app.update();assert_eq!(model(&app).quantity.unwrap().amount,6);
        edge(&mut app,503,TouchPhase::Canceled,meter_at(bounds,0.0));app.update();app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,6,"cancel retains last applied amount, matching existing held meter semantics");
        assert_eq!(offered(&app),0);
    }
}
#[test]
fn ordinary_quantity_meter_foreign_amount_and_same_queue_prompt_replacement_retire_old_owner() {
    for native in [false,true] {for replace in [false,true] {
        let (mut app,_)=meter_scene(native,8);let bounds=meter_bounds(&mut app,native);let p=meter_at(bounds,0.25);
        edge(&mut app,505,TouchPhase::Started,p);app.update();assert_eq!(model(&app).quantity.unwrap().amount,2);
        if replace {
            app.world_mut().write_message(MenuInput(Command::AmountCancel));
            app.world_mut().write_message(MenuInput(Command::One));
        } else {app.world_mut().write_message(MenuInput(Command::Amount(4)));}
        edge(&mut app,505,TouchPhase::Moved,meter_at(bounds,0.75));app.update();
        let want=if replace {8}else{4};assert_eq!(model(&app).quantity.unwrap().amount,want);
        edge(&mut app,505,TouchPhase::Ended,meter_at(bounds,1.));app.update();app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,want);assert_eq!(offered(&app),0);
        let bounds=meter_bounds(&mut app,native);edge(&mut app,506,TouchPhase::Started,meter_at(bounds,0.75));app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,6,"real neutral allows a new reviewed prompt owner");
        edge(&mut app,506,TouchPhase::Ended,meter_at(bounds,0.75));app.update();
        assert_eq!(offered(&app),0);
    }}
}
#[derive(Resource,Default)]struct MeterToggle(bool);
fn meter_toggle(world:&mut World) {
    if !std::mem::take(&mut world.resource_mut::<MeterToggle>().0){return}
    let row=world.resource::<crate::settings::Settings>().index(KEY).unwrap();
    assert_eq!(crate::settings::flip(world,row),Some(false));assert_eq!(crate::settings::flip(world,row),Some(true));
}
#[derive(Resource,Default)]struct MeterLate(Option<bool>);
fn meter_late_gate(world:&mut World) {
    let Some(window_case)=world.resource_mut::<MeterLate>().0.take() else{return};
    if window_case {let w=world.resource::<Fixture>().0.window;world.get_mut::<Window>(w).unwrap().focused=false;}
    else {world.resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.open_menu(fnv_game::game::menus::MESSAGE);}
}
#[test]
fn ordinary_quantity_meter_late_modal_window_and_registered_off_on_refuse_then_fresh_contact_recovers() {
    for native in [false,true] {for cause in ["modal","window","toggle"] {
        let (mut app,_)=meter_scene(native,8);let bounds=meter_bounds(&mut app,native);
        app.init_resource::<MeterLate>().add_systems(Update,meter_late_gate.after(normal::Dispatch).before(crate::menu::MenuSystems));
        edge(&mut app,507,TouchPhase::Started,meter_at(bounds,0.25));
        if cause=="toggle" {
            // Both callbacks occur after raw admission, before Dispatch/effect.
            app.insert_resource(MeterToggle(true)).add_systems(Update,meter_toggle.before(normal::Dispatch));
        } else {app.world_mut().resource_mut::<MeterLate>().0=Some(cause=="window");}
        app.update();assert_eq!(model(&app).quantity.unwrap().amount,8);
        edge(&mut app,507,TouchPhase::Ended,meter_at(bounds,0.75));app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,8);assert_eq!(offered(&app),0);
        let w=app.world().resource::<Fixture>().0.window;app.world_mut().get_mut::<Window>(w).unwrap().focused=true;
        let mode=model(&app).mode;crate::script::engine_menu_opened(Some(&mut app.world_mut().resource_mut::<crate::script::ScriptState>()),mode);
        app.update();app.update();let bounds=meter_bounds(&mut app,native);
        edge(&mut app,508,TouchPhase::Started,meter_at(bounds,0.75));app.update();assert_eq!(model(&app).quantity.unwrap().amount,6);
        edge(&mut app,508,TouchPhase::Ended,meter_at(bounds,0.75));app.update();
    }}
}
#[test]
fn ordinary_quantity_meter_source_replacement_and_mouse_takeover_preserve_last_amount() {
    for native in [false,true] {for hardware in [false,true] {
        let (mut app,_)=meter_scene(native,8);let bounds=meter_bounds(&mut app,native);
        edge(&mut app,509,TouchPhase::Started,meter_at(bounds,0.5));app.update();meter_paint(&mut app,native);
        assert_eq!(model(&app).quantity.unwrap().amount,4);
        if hardware {
            meter_hardware(&mut app,Vec2::new(630.,350.),true);
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        } else if native {
            {let mut script=app.world_mut().resource_mut::<crate::script::ScriptState>();let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;
                assert!(ui.set_float("QuantityMenu/NOGLOW_BRANCH/QM_MainRect/QM_AmountMeter/x",90.));ui.update();}
            assert_ne!(meter_bounds(&mut app,true),bounds,"actual native layout changed independently of owned amount");
        } else {
            let old={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
            app.world_mut().resource_mut::<ActiveMenu>().0.as_mut().unwrap().status=Some("Authored repaint".into());app.update();
            let next={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
            assert_ne!(old,next,"the ordinary painter actually replaced this meter without an owned Amount");
        }
        edge(&mut app,509,TouchPhase::Ended,meter_at(bounds,0.75));app.update();app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,4);assert_eq!(offered(&app),0);
        if hardware {
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));meter_hardware(&mut app,Vec2::new(630.,350.),false);
            let bounds=meter_bounds(&mut app,native);normal_tap_at(&mut app,meter_at(bounds,0.75));
            assert_eq!(model(&app).quantity.unwrap().amount,4,"first return touch only restores after real mouse ownership");
        }
        app.update();app.update();let bounds=meter_bounds(&mut app,native);let fresh=meter_at(bounds,0.75);
        edge(&mut app,520,TouchPhase::Started,fresh);app.update();assert_eq!(model(&app).quantity.unwrap().amount,6,"fresh current-source meter owner recovers after neutral/return");
        edge(&mut app,520,TouchPhase::Ended,fresh);app.update();assert_eq!(offered(&app),0);
    }}
}
#[test]
fn ordinary_native_meter_subslop_outer_end_clamps_without_clicking_adjacent_confirm() {
    let (mut app,stock)=meter_scene(true,1024);let bounds=meter_bounds(&mut app,true);
    let p=Vec2::new(bounds.max.x-2.,bounds.center().y);let end=Vec2::new(bounds.max.x+7.,bounds.center().y);
    assert!(p.distance(end)<12. && !bounds.contains(end));
    edge(&mut app,510,TouchPhase::Started,p);app.update();
    assert!(model(&app).quantity.unwrap().amount<1024);
    edge(&mut app,510,TouchPhase::Ended,end);app.update();app.update();
    assert_eq!(model(&app).quantity.unwrap().amount,1024);assert_eq!(offered(&app),0);
    assert_eq!(count(&mut app,stock,GOODS),1024);assert_eq!(count(&mut app,PLAYER,CAPS),1000);
}
#[test]
fn ordinary_native_meter_stationary_end_retains_real_native_click_pulse_once() {
    let (mut app,_)=meter_scene(true,8);let bounds=meter_bounds(&mut app,true);let p=meter_at(bounds,0.5);
    edge(&mut app,511,TouchPhase::Started,p);app.update();assert_eq!(model(&app).quantity.unwrap().amount,4);
    assert!(!app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.ui_pointer.pending(),"live typed sample did not fabricate a native click pulse");
    edge(&mut app,511,TouchPhase::Ended,p);app.update();
    let game=&app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game;
    assert!(game.ui_pointer.pending(),"inside stationary End still used existing native click adapter");
    app.update();assert_eq!(model(&app).quantity.unwrap().amount,4);assert_eq!(offered(&app),0);
}

#[test]
fn ordinary_native_meter_same_geometry_new_prompt_source_refuses_old_id_then_fresh_works() {
    let (mut app,_)=meter_scene(true,8);let bounds=meter_bounds(&mut app,true);
    edge(&mut app,512,TouchPhase::Started,meter_at(bounds,0.5));app.update();meter_paint(&mut app,true);
    assert_eq!(model(&app).quantity.unwrap().amount,4);
    {
        let mut script=app.world_mut().resource_mut::<crate::script::ScriptState>();let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;
        let old=ui.menu(crate::tiles::menus::QUANTITY_MENU).unwrap();ui.unload_tile(old);
        meter_load_native(ui);ui.update();
        assert_ne!(ui.menu(crate::tiles::menus::QUANTITY_MENU),Some(old),"actual native source replacement uses new live IDs");
    }
    assert_eq!(meter_bounds(&mut app,true),bounds,"same geometry alone is insufficient source identity");
    edge(&mut app,512,TouchPhase::Ended,meter_at(bounds,0.75));app.update();app.update();
    assert_eq!(model(&app).quantity.unwrap().amount,4);assert_eq!(offered(&app),0);
    let bounds=meter_bounds(&mut app,true);edge(&mut app,513,TouchPhase::Started,meter_at(bounds,0.75));app.update();
    assert_eq!(model(&app).quantity.unwrap().amount,6);
    edge(&mut app,513,TouchPhase::Ended,meter_at(bounds,0.75));app.update();assert_eq!(offered(&app),0);
}

#[test]
fn ordinary_quantity_meter_other_native_intent_retires_physical_hold_then_fresh_press_recovers() {
    for replacement in [false,true] {
        let (mut app,_)=meter_scene(false,8);let bounds=meter_bounds(&mut app,false);
        meter_hardware(&mut app,meter_at(bounds,0.25),true);
        assert_eq!(model(&app).quantity.unwrap().amount,2);
        let old={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
        if replacement {
            app.world_mut().write_message(MenuInput(Command::AmountCancel));
            app.world_mut().write_message(MenuInput(Command::One));
        } else {app.world_mut().write_message(MenuInput(Command::AmountStep(1)));}
        app.update();
        let want=if replacement {8}else{3};assert_eq!(model(&app).quantity.unwrap().amount,want);
        let current={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
        assert_ne!(old,current,"another real consumer intent replaces the physical source");
        let bounds=meter_bounds(&mut app,false);let p=meter_at(bounds,0.75);
        let window=app.world().resource::<Fixture>().0.window;
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(p));app.update();
        assert_eq!(model(&app).quantity.unwrap().amount,want,"a held mouse cannot restart on a replacement prompt");
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        meter_hardware(&mut app,p,false);meter_hardware(&mut app,p,true);
        assert_eq!(model(&app).quantity.unwrap().amount,6,"release and a real fresh press recover");
        meter_hardware(&mut app,p,false);assert_eq!(offered(&app),0);assert_eq!(count(&mut app,PLAYER,CAPS),1000);
    }
}

#[test]
fn ordinary_physical_meter_context_changes_retire_hold_and_noop_size_keeps_live_drag() {
    for cause in ["scale","density","camera","window","resize","layout","transform","size","roundtrip","new-max","status","focus","noop"] {
        let (mut app,stock)=meter_scene(false,8);
        crate::menu_readability::register(&mut app,crate::pause::GROUP_MENUS);
        app.update();app.update();let bounds=meter_bounds(&mut app,false);
        meter_hardware(&mut app,meter_at(bounds,0.25),true);
        assert_eq!(model(&app).quantity.unwrap().amount,2,"actual physical Start before context change: {cause}");
        let f=app.world().resource::<Fixture>().0;
        match cause {
            "scale"=>meter_display(&mut app,1.0,1.5),
            "density"=>meter_display(&mut app,2.0,1.0),
            "camera"=>{
                app.world_mut().entity_mut(f.camera).remove::<bevy::ui::IsDefaultUiCamera>();
                app.world_mut().get_mut::<Camera>(f.camera).unwrap().is_active=false;
                let camera=app.world_mut().spawn((Camera2d,bevy::ui::IsDefaultUiCamera,Camera {
                    computed:ComputedCameraValues {target_info:Some(RenderTargetInfo {
                        physical_size:f.size.as_uvec2(),scale_factor:1.0}),..default()},..default()
                },RenderTarget::Window(WindowRef::Entity(f.window)))).id();
                app.world_mut().resource_mut::<Fixture>().0.camera=camera;
            }
            "window"=>{
                app.world_mut().entity_mut(f.window).remove::<PrimaryWindow>();
                let window=app.world_mut().spawn((Window {resolution:(640,360).into(),focused:true,..default()},PrimaryWindow)).id();
                *app.world_mut().get_mut::<RenderTarget>(f.camera).unwrap()=RenderTarget::Window(WindowRef::Entity(window));
                app.world_mut().resource_mut::<Fixture>().0.window=window;
            }
            "resize"=>{
                app.world_mut().get_mut::<Window>(f.window).unwrap().resolution=(800,450).into();
                app.world_mut().get_mut::<Camera>(f.camera).unwrap().computed.target_info=Some(RenderTargetInfo {
                    physical_size:UVec2::new(800,450),scale_factor:1.0});
                app.world_mut().resource_mut::<Fixture>().0.size=Vec2::new(800.0,450.0);
            }
            "layout"=>{
                let entity={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
                let mut node=app.world_mut().get_mut::<Node>(entity).unwrap();
                let Val::Vh(width)=node.width else{panic!("actual meter uses interface viewport units")};
                node.width=Val::Vh(width+2.0);
            }
            "transform"=>{
                let entity={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
                app.world_mut().get_mut::<bevy::ui::UiTransform>(entity).unwrap().translation=bevy::ui::Val2::px(12.0,0.0);
                app.update();
                let moved=meter_geometry(&mut app,false);
                assert!((moved.center().x-bounds.center().x).abs()>5.0,"actual layout applied the changed transform before the next held sample");
                app.world_mut().get_mut::<Window>(f.window).unwrap().set_cursor_position(Some(meter_at(bounds,0.75)));
            }
            "size"|"roundtrip"|"noop"=>{
                let row=app.world().resource::<crate::settings::Settings>().index("barter-menu-size-v1").unwrap();
                let current=(app.world().resource::<crate::settings::Settings>().list()[row].slider.as_ref().unwrap().get)(app.world());
                let part=if cause=="noop" {current}else{1.0};
                assert!((crate::settings::set_slider(app.world_mut(),row,part).unwrap()-part).abs()<0.002);
                if cause=="roundtrip" {assert!((crate::settings::set_slider(app.world_mut(),row,current).unwrap()-current).abs()<0.002);}
            }
            "new-max"=>{
                let next={let mut sim=app.world_mut().resource_mut::<crate::sim::SimState>();
                    sim.world.add_item(stock,GOODS,8).unwrap();
                    MenuModel::open_barter(&mut sim.world,stock,stock,CAPS,100.0,0).unwrap()};
                let index=next.list(Side::Container).rows.iter().position(|r|r.item==GOODS).unwrap();
                app.world_mut().resource_mut::<ActiveMenu>().0=Some(next);
                app.world_mut().write_message(MenuInput(Command::Hover(Side::Container,index)));
                app.world_mut().write_message(MenuInput(Command::One));
            }
            "status"=>{
                app.world_mut().resource_mut::<ActiveMenu>().0.as_mut().unwrap().status=Some("Review this changed quote".into());
                app.world_mut().get_mut::<Window>(f.window).unwrap().set_cursor_position(Some(meter_at(bounds,0.75)));
            }
            "focus"=>{app.world_mut().get_mut::<Window>(f.window).unwrap().focused=false;}
            _=>unreachable!(),
        }
        for _ in 0..3 {app.update();}
        let want=if cause=="new-max" {16}else{2};
        assert_eq!(model(&app).quantity.unwrap().amount,want,"context change cannot apply a stale held sample: {cause}");
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left),"context retirement preserves real hardware: {cause}");
        if cause=="focus" {app.world_mut().get_mut::<Window>(f.window).unwrap().focused=true;app.update();app.update();}
        let bounds=meter_bounds(&mut app,false);let p=meter_at(bounds,0.75);
        let window=app.world().resource::<Fixture>().0.window;
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(p));app.update();
        if cause=="noop" {
            assert_eq!(model(&app).quantity.unwrap().amount,6,"same effective setting preserves the original held drag");
        } else {
            assert_eq!(model(&app).quantity.unwrap().amount,want,"held mouse cannot restart after context retirement: {cause}");
            let world=app.world_mut();let mut q=world.query_filtered::<&crate::ui_pointer::PointerState,With<crate::menu::AmountMeter>>();
            assert_ne!(*q.single(world).unwrap(),crate::ui_pointer::PointerState::Pressed);
        }
        meter_hardware(&mut app,p,false);meter_hardware(&mut app,p,true);
        assert_eq!(model(&app).quantity.unwrap().amount,if cause=="new-max" {12}else{6},"real release/fresh press recovers in current painted geometry: {cause}");
        meter_hardware(&mut app,p,false);assert_eq!(offered(&app),0);assert_eq!(count(&mut app,PLAYER,CAPS),1000);
    }
}

#[test]
fn ordinary_physical_meter_partial_viewport_keeps_original_fresh_click_without_retained_drag() {
    let (mut app,_)=meter_scene(false,8);
    let f=app.world().resource::<Fixture>().0;
    app.world_mut().get_mut::<Camera>(f.camera).unwrap().viewport=Some(bevy::camera::Viewport {
        physical_position:UVec2::ZERO,physical_size:UVec2::new(320,180),..default()
    });
    for _ in 0..3 {app.update();}
    assert_eq!(app.world().get::<Camera>(f.camera).unwrap().logical_viewport_size(),Some(Vec2::new(320.0,180.0)));
    let bounds=meter_bounds(&mut app,false);let p=meter_at(bounds,0.25);
    meter_hardware(&mut app,p,true);
    assert_eq!(model(&app).quantity.unwrap().amount,2,"an actual painted partial-viewport click retains the original consumer");
    meter_hardware(&mut app,p,false);
    let bounds=meter_bounds(&mut app,false);let p=meter_at(bounds,0.75);
    meter_hardware(&mut app,p,true);
    assert_eq!(model(&app).quantity.unwrap().amount,6,"fresh click works without a qualified full-window retention stamp");
    meter_hardware(&mut app,p,false);
    assert_eq!(offered(&app),0);assert_eq!(count(&mut app,PLAYER,CAPS),1000);
}

#[test]
fn ordinary_container_meter_keeps_original_fresh_mouse_route_across_unrelated_barter_size_change() {
    let (mut app,stock)=meter_scene(false,8);
    crate::menu_readability::register(&mut app,crate::pause::GROUP_MENUS);
    let next={let mut sim=app.world_mut().resource_mut::<crate::sim::SimState>();
        MenuModel::open_container(&mut sim.world,stock).unwrap()};
    let index=next.list(Side::Container).rows.iter().position(|r|r.item==GOODS).unwrap();
    crate::script::engine_menu_opened(Some(&mut app.world_mut().resource_mut::<crate::script::ScriptState>()),next.mode);
    app.world_mut().resource_mut::<ActiveMenu>().0=Some(next);
    app.world_mut().write_message(MenuInput(Command::Hover(Side::Container,index)));
    app.world_mut().write_message(MenuInput(Command::One));
    for _ in 0..3 {app.update();}
    assert!(matches!(model(&app).mode,Mode::Container(_)));
    assert_eq!(model(&app).quantity.unwrap().max,8);
    let bounds={let world=app.world_mut();
        let mut q=world.query_filtered::<(&ComputedNode,&bevy::ui::UiGlobalTransform),With<crate::menu::AmountMeter>>();
        let (node,at)=q.single(world).unwrap();Rect::from_center_size(at.translation,node.size())};
    meter_hardware(&mut app,meter_at(bounds,0.25),true);
    assert_eq!(model(&app).quantity.unwrap().amount,2);
    app.update();
    let source={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
    let row=app.world().resource::<crate::settings::Settings>().index("barter-menu-size-v1").unwrap();
    assert_eq!(crate::settings::set_slider(app.world_mut(),row,1.0),Some(1.0));
    assert_eq!(app.world().resource::<crate::menu_readability::MenuReadability>().size(crate::menu_readability::MenuKind::Barter),2.0);
    app.update();
    let current={let world=app.world_mut();let mut q=world.query_filtered::<Entity,With<crate::menu::AmountMeter>>();q.single(world).unwrap()};
    assert_eq!(current,source,"unrelated Barter setting cannot replace an unchanged container source");
    assert_eq!(model(&app).quantity.unwrap().amount,2);
    assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    let p=meter_at(bounds,0.75);meter_hardware(&mut app,p,false);meter_hardware(&mut app,p,true);
    assert_eq!(model(&app).quantity.unwrap().amount,6,"the original fresh physical admission stays available");
    meter_hardware(&mut app,p,false);
    assert_eq!(count(&mut app,stock,GOODS),8);assert_eq!(count(&mut app,PLAYER,CAPS),1000);
}
