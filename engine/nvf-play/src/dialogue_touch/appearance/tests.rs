//! Real enhanced draw/Taffy/registered-settings/raw-release controls; no game data/GPU.
use super::*;
use super::super::{compact_tests::{layout_scene,rows,send,settle},Action,Kind,Root,Viewport,TouchDialogue,TouchFrame,Gestures};
use bevy::input::touch::TouchPhase;
use bevy::ui::{ComputedNode,UiGlobalTransform,ScrollPosition};
fn index(app:&App,key:&str)->usize {app.world().resource::<crate::settings::Settings>().index(key).unwrap()}
fn theme(app:&mut App,value:Theme) {
    let row=index(app,THEME_KEY);
    let changed=app.world().resource::<Appearance>().theme!=value;
    assert_eq!(crate::settings::pick(app.world_mut(),row,value.name()),changed);
}
fn slider(app:&mut App,key:&str,value:f32) {let row=index(app,key);crate::settings::set_slider(app.world_mut(),row,value);}
fn root(app:&mut App)->Entity {app.world_mut().query_filtered::<Entity,With<Root>>().single(app.world()).unwrap()}
fn point(app:&mut App,kind:Kind)->Vec2 {
    let mut q=app.world_mut().query::<(&Action,&ComputedNode,&UiGlobalTransform)>();
    let (_,_,t)=q.iter(app.world()).find(|(a,_,_)|a.kind==kind).unwrap();
    t.transform_point2(Vec2::ZERO)
}
fn tap(app:&mut App,window:Entity,id:u64,at:Vec2) {
    send(app,window,id,TouchPhase::Started,at);app.update();
    send(app,window,id,TouchPhase::Ended,at);app.update();
}
fn text(app:&mut App,kind:fn(&Paint)->bool)->Vec<(String,Color)> {
    let mut q=app.world_mut().query::<(&Paint,&Text,&TextColor)>();
    q.iter(app.world()).filter(|(p,_,_)|kind(p)).map(|(_,t,c)|(t.0.clone(),c.0)).collect()
}
#[test]
fn field_notes_repaints_without_moving_rows_scroll_selection_or_targets() {
    for size in [Vec2::new(640.,360.),Vec2::new(792.,363.)] {
        let (mut app,_) = layout_scene(true,size,1.);
        app.world_mut().resource_mut::<crate::dialogue_menu::DialogueMenu>().selected=3;
        settle(&mut app);
        let before_root=root(&mut app);
        let viewport=app.world_mut().query_filtered::<Entity,With<Viewport>>().single(app.world()).unwrap();
        let before_scroll=app.world().get::<ScrollPosition>(viewport).unwrap().y;
        let before_rows=rows(&mut app);
        assert_eq!(before_rows.len(),8);
        slider(&mut app,BACKGROUND_KEY,0.31);slider(&mut app,FRAME_KEY,0.42);app.update();
        assert_eq!(root(&mut app),before_root);
        assert_eq!(rows(&mut app),before_rows);
        assert_eq!(app.world().get::<ScrollPosition>(viewport).unwrap().y,before_scroll);
        assert_eq!(app.world().resource::<crate::dialogue_menu::DialogueMenu>().selected,3);
        assert!(rows(&mut app).iter().all(|(_,r)|r.height()>=48.));
        let mut q=app.world_mut().query::<(&Paint,&BackgroundColor)>();
        assert_eq!(q.iter(app.world()).filter(|(p,c)|matches!(p,Paint::Trim(_))&&c.0.alpha()>0.).count(),1);
    }
}
#[test]
fn normal_and_missing_specialized_providers_publish_only_the_native_choice_targets() {
    use crate::dialogue_menu::TouchNativeRoot;
    let (mut app,window)=layout_scene(true,Vec2::new(792.,363.),1.);
    assert_eq!(Appearance::default().theme, Theme::Minimal);
    app.init_resource::<Assets<Image>>();
    let _ = app.world_mut().remove_resource::<crate::pad::PadGlyphs>();
    theme(&mut app,Theme::Minimal);settle(&mut app);
    assert_eq!(app.world_mut().query_filtered::<Entity,With<TouchNativeRoot>>().iter(app.world()).count(),0);
    app.init_resource::<crate::pad::PadGlyphs>();
    app.world_mut().resource_mut::<TouchDialogue>().confirm=false;
    let expected=app.world().resource::<crate::dialogue_menu::DialogueMenu>().list.clone();
    for theme_value in [Theme::Minimal,Theme::ClassicGlass,Theme::RetroConsole,Theme::WristTerminal] {
        theme(&mut app,theme_value);settle(&mut app);
        assert!(app.world().get_resource::<nvf_assets::GameData>().is_none());
        assert!(app.world().get_resource::<crate::dialogue_device::DeviceView>().is_none());
        assert!(app.world().resource::<super::super::classic_presenter::Surface>().0.is_none());
        assert_eq!(app.world_mut().query_filtered::<Entity,With<Root>>().iter(app.world()).count(),0);
        let native=app.world_mut().query_filtered::<Entity,With<TouchNativeRoot>>().single(app.world()).unwrap();
        let mut q=app.world_mut().query::<(Entity,&Action)>();
        let targets: Vec<_>=q.iter(app.world()).filter_map(|(entity,action)|match action.kind {
            Kind::Choice(index)=>Some((entity,index,action.list.clone())),_=>None,
        }).collect();
        let mut indices=Vec::new();
        for (entity,index,list) in targets {
            assert_eq!(list,expected);
            let mut owner=entity;
            while owner!=native {owner=app.world().get::<ChildOf>(owner).expect("each choice belongs to the native painter").parent();}
            indices.push(index);
        }
        indices.sort_unstable();
        assert_eq!(indices,(0..8).collect::<Vec<_>>());
        let at=point(&mut app,Kind::Choice(0));tap(&mut app,window,3060,at);
        assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
    }
    theme(&mut app,Theme::SplitPanel);settle(&mut app);
    assert_eq!(app.world_mut().query_filtered::<Entity,With<TouchNativeRoot>>().iter(app.world()).count(),0);
    assert_eq!(rows(&mut app).len(),8);
    root(&mut app);
}

#[test]
fn zero_surface_opacity_keeps_real_text_and_confirm_target_and_released_choice_working() {
    let (mut app,window)=layout_scene(true,Vec2::new(792.,363.),1.);
    slider(&mut app,BACKGROUND_KEY,0.);slider(&mut app,FRAME_KEY,0.);settle(&mut app);
    let mut q=app.world_mut().query::<(&Paint,Option<&BackgroundColor>,Option<&BorderColor>)>();
    let mut focused=0;
    for (kind,back,border) in q.iter(app.world()) {
        if let Some(c)=back {
            if matches!(kind,Paint::Focus{index:0,..}) {assert_eq!(c.0.alpha(),1.);focused+=1;}else{assert_eq!(c.0.alpha(),0.);}
        }
        if let Some(c)=border {assert_eq!(c.top.alpha(),0.);}
    }
    assert_eq!(focused,1,"the current selection keeps an independent high-contrast cue");
    for (_,color) in text(&mut app,|p|matches!(p,Paint::Speaker|Paint::ChoiceText{..}|Paint::Caption)) {assert_eq!(color.alpha(),1.);}
    let first=point(&mut app,Kind::Choice(0));tap(&mut app,window,3001,first);
    assert_eq!(app.world().resource::<TouchFrame>().asked.pick,None);
    assert_eq!(app.world().resource::<Gestures>().armed,Some(0));
    let confirm=point(&mut app,Kind::Confirm);tap(&mut app,window,3002,confirm);
    assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
    assert!(text(&mut app,|p|matches!(p,Paint::Caption)).iter().any(|(s,_)|s=="Confirm selection"));
}
#[test]
fn hiding_only_hints_keeps_speech_choices_and_required_confirm_caption() {
    let (mut app,window)=layout_scene(true,Vec2::new(792.,363.),1.);
    app.world_mut().resource_mut::<crate::dialogue_menu::DialogueMenu>().view.as_mut().unwrap().line=Some("An authored speaking line remains visible.".into());
    settle(&mut app);
    app.world_mut().resource_mut::<Appearance>().hints=true;
    let row=index(&app,HINTS_KEY);assert_eq!(crate::settings::flip(app.world_mut(),row),Some(false));settle(&mut app);
    assert!(app.world().resource::<TouchDialogue>().enabled);
    let mut q=app.world_mut().query::<(&Paint,&Node,&ComputedNode)>();
    for (p,n,computed) in q.iter(app.world()) {if matches!(p,Paint::Hint) {assert_eq!(n.display,Display::None);assert_eq!(computed.size(),Vec2::ZERO);}}
    assert_eq!(text(&mut app,|p|matches!(p,Paint::Speech))[0].0,"An authored speaking line remains visible.");
    assert_eq!(text(&mut app,|p|matches!(p,Paint::ChoiceText{..})).len(),8);
    assert!(text(&mut app,|p|matches!(p,Paint::Caption)).iter().any(|(s,_)|s=="Select a choice first"));
    let at=point(&mut app,Kind::Choice(0));tap(&mut app,window,3003,at);
    assert_eq!(app.world().resource::<Gestures>().armed,Some(0));
    let confirm=point(&mut app,Kind::Confirm);tap(&mut app,window,3004,confirm);
    assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
}
#[test]
fn hints_geometry_change_retires_old_release_then_fresh_real_hit_answers() {
    let (mut app,window)=layout_scene(true,Vec2::new(640.,360.),1.);
    app.world_mut().resource_mut::<TouchDialogue>().confirm=false;app.world_mut().resource_mut::<Appearance>().hints=true;settle(&mut app);
    let before=point(&mut app,Kind::Choice(0));
    send(&mut app,window,3005,TouchPhase::Started,before);app.update();
    app.world_mut().resource_mut::<Appearance>().hints=true;
    let row=index(&app,HINTS_KEY);assert_eq!(crate::settings::flip(app.world_mut(),row),Some(false));
    send(&mut app,window,3005,TouchPhase::Ended,before);app.update();
    assert_eq!(app.world().resource::<TouchFrame>().asked.pick,None);
    settle(&mut app);let after=point(&mut app,Kind::Choice(0));tap(&mut app,window,3006,after);
    assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
}
#[test]
fn appearance_ini_restores_independently_of_enabled_sizes_confirmation_and_side() {
    let path=std::env::temp_dir().join(format!("nvf-dialogue-theme-{}-{}.ini",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let (mut saved,_)=layout_scene(true,Vec2::new(792.,363.),1.);
    saved.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
    theme(&mut saved,Theme::SplitPanel);slider(&mut saved,BACKGROUND_KEY,0.37);slider(&mut saved,FRAME_KEY,0.28);
    saved.world_mut().resource_mut::<Appearance>().hints=true;
    let row=index(&saved,HINTS_KEY);assert_eq!(crate::settings::flip(saved.world_mut(),row),Some(false));
    slider(&mut saved,"dialogue-choice-text-size",0.6);slider(&mut saved,"dialogue-subtitle-text-size",0.4);
    let row=index(&saved,"dialogue-touch-choice");assert!(crate::settings::pick(saved.world_mut(),row,"Direct tap"));
    let row=index(&saved,crate::dialogue_split::KEY);
    // The host fixture's existing default may already be Standard; choose a different valid side first.
    let current=saved.world().resource::<crate::dialogue_split::Options>().side;
    let side=if current==crate::dialogue_split::Side::Left {"Speaker right"}else{"Speaker left"};
    assert!(crate::settings::pick(saved.world_mut(),row,side));
    let tint=index(&saved,TINT_KEY);assert!(crate::settings::pick(saved.world_mut(),tint,"Warm amber"));
    theme(&mut saved,Theme::WristTerminal);
    let row=index(&saved,"touch-dialogue");assert_eq!(crate::settings::flip(saved.world_mut(),row),Some(false));
    let expected=*saved.world().resource::<Appearance>();let expected_text=*saved.world().resource::<TouchDialogue>();let expected_side=*saved.world().resource::<crate::dialogue_split::Options>();
    let (mut restored,window)=layout_scene(true,Vec2::new(792.,363.),1.);
    restored.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
    restored.insert_resource(crate::settings::SettingsFile(Some(path.clone())));crate::settings::apply_stored(restored.world_mut());settle(&mut restored);
    assert_eq!(*restored.world().resource::<Appearance>(),expected);
    assert_eq!(*restored.world().resource::<TouchDialogue>(),expected_text);
    assert_eq!(restored.world().resource::<crate::dialogue_split::Options>().side,expected_side.side);
    assert_eq!(restored.world_mut().query_filtered::<Entity,With<Root>>().iter(restored.world()).count(),0);
    assert_eq!(restored.world_mut().query_filtered::<Entity,With<crate::dialogue_menu::TouchNativeRoot>>().iter(restored.world()).count(),0);
    let row=index(&restored,"touch-dialogue");assert_eq!(crate::settings::flip(restored.world_mut(),row),Some(true));settle(&mut restored);
    assert_eq!(*restored.world().resource::<Appearance>(),expected);
    let at=point(&mut restored,Kind::Choice(0));tap(&mut restored,window,3007,at);
    assert_eq!(restored.world().resource::<TouchFrame>().asked.pick,Some(0));
    std::fs::remove_file(path).unwrap();
}
#[test]
fn field_notes_keeps_long_choice_text_order_and_short_and_darkened_flags() {
    let (mut app,_)=layout_scene(true,Vec2::new(792.,363.),1.);
    {
        let mut menu=app.world_mut().resource_mut::<crate::dialogue_menu::DialogueMenu>();
        for (i,row) in menu.view.as_mut().unwrap().choices.iter_mut().enumerate() {
            row.label=format!("Response {i}: an authored longer option which wraps while preserving its exact offered order.");row.short=i==1;row.darkened=i==2;
        }
    }
    app.world_mut().resource_mut::<crate::dialogue_menu::DialogueMenu>().selected=1;
    settle(&mut app);
    let expected:Vec<_>=app.world().resource::<crate::dialogue_menu::DialogueMenu>().view.as_ref().unwrap().choices.iter().map(|c|c.label.clone()).collect();
    {
        app.update();
        assert_eq!(text(&mut app,|p|matches!(p,Paint::ChoiceText{..})).iter().map(|(s,_)|s.clone()).collect::<Vec<_>>(),expected);
        assert!(rows(&mut app).iter().all(|(_,r)|r.height()>=48.));
        let menu=app.world().resource::<crate::dialogue_menu::DialogueMenu>();
        assert!(menu.view.as_ref().unwrap().choices[1].short);assert!(menu.view.as_ref().unwrap().choices[2].darkened);
        assert!(app.world().resource::<TouchDialogue>().confirm);
        let mut q=app.world_mut().query::<(&Paint,&BorderColor)>();
        let (_,border)=q.iter(app.world()).find(|(p,_)|matches!(p,Paint::Choice{index:1,..})).unwrap();
        assert_eq!(border.top,Color::NONE,"responses have no box border; unmet check uses the persistent focus cue");
    }
}

#[cfg(not(target_os="android"))]
#[test]
fn actual_live_hud_palette_is_applied_once_after_generic_recolour() {
    const CHILD:&str="NVF_DIALOGUE_PALETTE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // HUD colour is process-global. Use this same test binary in an isolated
        // process so other parallel settings tests cannot observe our changes.
        let name=concat!(module_path!(),"::actual_live_hud_palette_is_applied_once_after_generic_recolour");
        let selector=name.split_once("::").unwrap().1;
        let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",selector,"--test-threads=1"]).env(CHILD,"1").spawn().unwrap();
        let started=std::time::Instant::now();
        let status=loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed()<std::time::Duration::from_secs(30) =>
                    std::thread::sleep(std::time::Duration::from_millis(10)),
                Ok(None) => {
                    let killed=child.kill();let reaped=child.wait();
                    panic!("isolated palette control exceeded 30s; own-child kill={killed:?}, reap={reaped:?}");
                }
                Err(error) => {
                    let killed=child.kill();let reaped=child.wait();
                    panic!("isolated palette wait failed: {error}; own-child kill={killed:?}, reap={reaped:?}");
                }
            }
        };
        assert!(status.success(),"isolated actual palette control failed");
        return;
    }
    let (mut app,_)=layout_scene(true,Vec2::new(792.,363.),1.);
    app.add_plugins(crate::ui_colour::UiColourPlugin);
    for rgb in [[200_u8,100,50],[100,50,25]] {
        for (key,value) in ["hud-red","hud-green","hud-blue"].into_iter().zip(rgb) {slider(&mut app,key,f32::from(value)/255.);}
        app.update();
        assert_eq!(crate::interface::hud_rgb(),rgb);
        let expected=crate::interface::tint(rgb,255.);
        assert!(text(&mut app,|p|matches!(p,Paint::Speaker|Paint::ChoiceText{darkened:false}|Paint::Caption)).iter().all(|(_,c)|*c==expected));
        let mut q=app.world_mut().query::<(&Paint,&BorderColor)>();
        let (_,border)=q.iter(app.world()).find(|(p,_)|matches!(p,Paint::Choice{index:0,..})).unwrap();
        assert_eq!(border.top,Color::NONE);
    }
}
#[test]
fn field_notes_keeps_speaker_side_camera_and_safe_area_at_both_sides() {
    fn publish(windows:Query<&Window,With<bevy::window::PrimaryWindow>>,mut cameras:Query<&mut Camera>) {
        let w=windows.single().unwrap();
        for mut c in &mut cameras {c.computed=bevy::camera::ComputedCameraValues{target_info:Some(bevy::camera::RenderTargetInfo{physical_size:UVec2::new(w.physical_width(),w.physical_height()),scale_factor:w.scale_factor()}),..default()};}
    }
    for side in [crate::dialogue_split::Side::Left,crate::dialogue_split::Side::Right] {
        let (mut app,_)=layout_scene(true,Vec2::new(792.,363.),1.);
        app.configure_sets(Update,(crate::PlaySet::Input,crate::PlaySet::Physics,crate::PlaySet::Activation,crate::PlaySet::Scripts,crate::PlaySet::Ui,crate::PlaySet::Sync).chain());
        let mut script=crate::script::ScriptState::over_no_plugins();
        script.rt.as_mut().unwrap().game.dialogue=Some(fnv_game::dialogue::Conversation{actor:fnv_sim::FormId(42),through:None,line:None,choices:vec![],topic_list:false,lines:1,then:None,ending:false});
        app.insert_resource(script).insert_resource(crate::dialogue_look::SpeakerFace(Some((42,[35.,180.,130.]))))
            .insert_resource(crate::dialogue_split::Options{side,device_side:crate::dialogue_split::DeviceSide::Right})
            .insert_resource(crate::touch_safe_area::SafeInsets{left:24.,top:12.,right:10.,bottom:18.})
            .add_systems(Update,publish.after(crate::PlaySet::Sync));
        let world=app.world_mut().spawn((crate::PlayerCamera,Camera3d::default(),Camera{order:1,..default()},Transform::from_xyz(0.,112.,0.))).id();
        settle(&mut app);
        let before_root=root(&mut app);
        let before=app.world().get::<Camera>(world).unwrap().viewport.clone();
        assert!(before.is_none());
        {
            app.update();
            assert_eq!(root(&mut app),before_root);
            assert_eq!(app.world().resource::<crate::dialogue_split::Options>().side,side);
            let frame=app.world().resource::<crate::dialogue_split::Frame>();
            let panel=frame.panel.expect("real split source supplies panel");
            assert!(panel.min.x>=24. && panel.min.y>=12. && panel.max.x<=782. && panel.max.y<=345.);
            assert_eq!(app.world().get::<UiTargetCamera>(before_root).unwrap().0,frame.ui_camera.unwrap());
            assert!(app.world().get::<Camera>(world).unwrap().viewport.is_none());
            let camera=app.world().get::<Transform>(world).unwrap();
            let view=camera.rotation.inverse()*(crate::space::to_bevy([35.,180.,130.])-camera.translation);
            assert!(if side==crate::dialogue_split::Side::Left {view.x<0.}else{view.x>0.});
            assert!(rows(&mut app).iter().all(|(_,r)|r.height()>=48.));
        }
    }
}

#[test]
fn maximum_background_setting_remains_translucent_while_frame_and_text_are_independent() {
    let (mut app,_)=layout_scene(true,Vec2::new(792.,363.),1.);
    {
        slider(&mut app,BACKGROUND_KEY,1.);slider(&mut app,FRAME_KEY,1.);app.update();
        assert_eq!(app.world().resource::<Appearance>().background,0.80);
        assert_eq!(app.world().resource::<Appearance>().frame,1.0);
        let mut q=app.world_mut().query::<(&Paint,&BackgroundColor)>();
        for (kind,back) in q.iter(app.world()) {if matches!(kind,Paint::Panel|Paint::Choice{..}|Paint::Confirm) {assert!(back.0.alpha()<=0.80);if matches!(kind,Paint::Panel){assert_eq!(back.0.alpha(),0.);}}}
        assert!(text(&mut app,|p|matches!(p,Paint::Speaker|Paint::ChoiceText{..}|Paint::Caption)).iter().all(|(_,c)|c.alpha()==1.));
    }
}
#[test]
fn field_notes_keeps_selected_unmet_check_cue_and_requirement_at_zero_frame_opacity() {
    let (mut app,window)=layout_scene(true,Vec2::new(792.,363.),1.);
    let label="[Authored check 3/5] This exact offered response remains visible.";
    {
        let mut menu=app.world_mut().resource_mut::<crate::dialogue_menu::DialogueMenu>();
        let row=&mut menu.view.as_mut().unwrap().choices[0];row.short=true;row.label=label.into();
    }
    let row=index(&app,"dialogue-touch-choice");assert!(crate::settings::pick(app.world_mut(),row,"Direct tap"));
    app.world_mut().resource_mut::<Appearance>().hints=true;
    let row=index(&app,HINTS_KEY);assert_eq!(crate::settings::flip(app.world_mut(),row),Some(false));
    slider(&mut app,BACKGROUND_KEY,0.);slider(&mut app,FRAME_KEY,0.);settle(&mut app);
    let before=rows(&mut app);
    {
        app.update();
        assert_eq!(rows(&mut app),before);
        let mut q=app.world_mut().query::<(&Paint,&BackgroundColor,Option<&BorderColor>)>();
        let (_,cue,_)=q.iter(app.world()).find(|(p,_,_)|matches!(p,Paint::Focus{index:0,short:true})).unwrap();
        assert_eq!(cue.0,check_line(),"functional unmet-check cue does not inherit cosmetic frame opacity");
        assert_eq!(cue.0.alpha(),1.);
        let mut borders=app.world_mut().query::<(&Paint,&BorderColor)>();
        let (_,border)=borders.iter(app.world()).find(|(p,_)|matches!(p,Paint::Choice{index:0,..})).unwrap();
        assert_eq!(border.top.alpha(),0.,"cosmetic border still obeys the frame slider");
        let required=text(&mut app,|p|matches!(p,Paint::ChoiceText{..}));
        assert!(required.iter().any(|(s,c)|s==label&&c.alpha()==1.));
        let at=point(&mut app,Kind::Choice(0));tap(&mut app,window,3010,at);
        assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0),"same actual offered-index route; game eligibility is not replaced");
        let saved=*app.world().resource::<Appearance>();
        let enabled=index(&app,"touch-dialogue");assert_eq!(crate::settings::flip(app.world_mut(),enabled),Some(false));app.update();
        tap(&mut app,window,3011,at);
        assert_eq!(app.world().resource::<TouchFrame>().asked.pick,None);
        assert_eq!(*app.world().resource::<Appearance>(),saved);
        assert_eq!(crate::settings::flip(app.world_mut(),enabled),Some(true));settle(&mut app);
        assert_eq!(*app.world().resource::<Appearance>(),saved);
        assert_eq!(rows(&mut app),before);
        let fresh=point(&mut app,Kind::Choice(0));tap(&mut app,window,3012,fresh);
        assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
    }
}
#[test]
fn minimum_admitted_preview_height_contains_real_taffy_text_at_all_ui_scales() {
    for ui_scale in [0.75,1.0,1.5] {
        let (mut app,_)=layout_scene(true,Vec2::new(792.,363.),1.);
        app.world_mut().resource_mut::<TouchDialogue>().enabled=false;
        app.world_mut().resource_mut::<UiScale>().0=ui_scale;
        settle(&mut app);
        let camera=app.world_mut().query_filtered::<Entity,With<Camera2d>>().single(app.world()).unwrap();
        app.world_mut().run_system_cached_with(|In(camera):In<Entity>,mut commands:Commands| {
            commands.spawn((Node{width:percent(100.),height:percent(100.),..default()},UiTargetCamera(camera))).with_children(|p| {
                preview(p,Node{position_type:PositionType::Absolute,left:px(0.),top:px(0.),width:crate::interface::u(200.),height:crate::interface::u(MIN_PREVIEW_UNITS),flex_direction:FlexDirection::Column,padding:UiRect::all(crate::interface::u(6.)),border:UiRect::all(crate::interface::u(1.)),overflow:Overflow::clip(),..default()});
            });
        },camera).unwrap();
        settle(&mut app);
        let preview=app.world_mut().query_filtered::<Entity,With<Preview>>().single(app.world()).unwrap();
        let node=app.world().get::<ComputedNode>(preview).unwrap();
        let transform=app.world().get::<UiGlobalTransform>(preview).unwrap();
        let bounds=Rect::from_center_size(transform.transform_point2(Vec2::ZERO),node.size());
        assert!(bounds.width()>0. && bounds.height()>0.);
        let mut descendants=vec![preview];let mut at=0;
        while at<descendants.len() {if let Some(children)=app.world().get::<Children>(descendants[at]) {descendants.extend(children.iter());}at+=1;}
        let mut count=0;
        for entity in descendants {
            if !app.world().get::<Paint>(entity).is_some_and(|p|matches!(p,Paint::Speaker|Paint::Caption)) {continue;}
            let node=app.world().get::<ComputedNode>(entity).unwrap();
            let transform=app.world().get::<UiGlobalTransform>(entity).unwrap();
            let text=Rect::from_center_size(transform.transform_point2(Vec2::ZERO),node.size());
            assert!(text.width()>0. && text.height()>0.,"real TextPlugin/Taffy measurement must have run");
            assert!(text.min.x>=bounds.min.x-0.01 && text.min.y>=bounds.min.y-0.01 && text.max.x<=bounds.max.x+0.01 && text.max.y<=bounds.max.y+0.01,"minimum admitted preview clips text at UI scale {ui_scale}: {text:?} outside {bounds:?}");
            count+=1;
        }
        assert_eq!(count,2,"both actual preview labels must be measured and contained");
    }
}

#[test]
fn field_notes_borderless_defaults_tints_and_selection_use_actual_draw_nodes() {
    assert!(!Appearance::default().hints);
    let (mut app,_)=layout_scene(true,Vec2::new(792.,363.),1.);
    let mut q=app.world_mut().query::<(&Paint,&Node,Option<&BackgroundColor>,Option<&BorderColor>)>();
    let mut focus=0;
    for (kind,node,back,border) in q.iter(app.world()) {
        if matches!(kind,Paint::Panel|Paint::Choice{..}|Paint::Confirm) {
            assert_eq!(node.border,UiRect::ZERO);
            if let Some(border)=border {assert_eq!(border.top.alpha(),0.);}
        }
        if matches!(kind,Paint::Panel) {assert_eq!(back.unwrap().0.alpha(),0.);}
        if matches!(kind,Paint::Focus{index:0,..}) {
            assert_eq!((node.left,node.bottom,node.width,node.height),(px(8.),px(3.),px(16.),px(1.)));
            focus+=1;
        }
    }
    assert_eq!(focus,1);
    let setting=index(&app,TINT_KEY);
    for name in ["Neutral","Warm amber","HUD colour"] {
        assert!(crate::settings::pick(app.world_mut(),setting,name));settle(&mut app);
        assert_eq!(tint_name(app.world()),name);
        let labels=text(&mut app,|p|matches!(p,Paint::ChoiceText{darkened:false}));
        assert_eq!(labels.len(),8);
        assert!(labels.iter().all(|(_,color)|*color==crate::interface::tint(crate::interface::hud_rgb(),255.)));
    }
}

#[test]
fn installed_cursor_uses_texture_alpha_instead_of_a_solid_procedural_backing() {
    use bevy::ecs::system::RunSystemOnce;
    let mut app=App::new();
    app.insert_resource(Appearance{theme:Theme::WristTerminal,..default()});
    app.init_resource::<crate::dialogue_menu::DialogueMenu>();
    let cursor=app.world_mut().spawn((Paint::Focus{index:0,short:false},Node::default(),BackgroundColor(Color::WHITE),ImageNode::default())).id();
    app.world_mut().run_system_once(paint).unwrap();
    assert_eq!(app.world().get::<BackgroundColor>(cursor).unwrap().0,Color::NONE);
    assert_eq!(app.world().get::<ImageNode>(cursor).unwrap().color,crate::interface::tint(crate::interface::hud_rgb(),255.));
}

#[test]
fn installed_cursor_first_insertion_frame_clears_backing_and_preserves_tint() {
    // Authored DDS through the production VFS/Pictures loader, never game assets.
    struct MarkerFiles(std::path::PathBuf);
    impl Drop for MarkerFiles {
        fn drop(&mut self) {
            let _=std::fs::remove_file(self.0.join("textures/interface/shared/scrollbar/vert_marker.dds"));
            for relative in ["textures/interface/shared/scrollbar","textures/interface/shared","textures/interface","textures",""] {
                let _=std::fs::remove_dir(self.0.join(relative));
            }
        }
    }
    let files=MarkerFiles(std::env::temp_dir().join(format!("nvf-dialogue-cursor-insertion-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    let folder=files.0.join("textures/interface/shared/scrollbar");
    std::fs::create_dir_all(&folder).unwrap();
    let mut dds=crate::interface::tests::authored_rgba_dds(2,2);
    dds[128..132].copy_from_slice(&[255,255,255,255]);
    std::fs::write(folder.join("vert_marker.dds"),dds).unwrap();
    let vfs=fnv_vfs::Vfs::open(&files.0,&[]).unwrap();
    let schema=fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
    let mut app=App::new();
    app.insert_resource(nvf_assets::GameData::new(std::sync::Arc::new(vfs),std::sync::Arc::new(schema)))
        .insert_resource(Appearance{theme:Theme::WristTerminal,..default()})
        .init_resource::<Assets<Image>>()
        .init_resource::<crate::dialogue_menu::DialogueMenu>()
        .add_systems(Update,(paint,instrument).chain());
    let cursor=app.world_mut().spawn((Paint::Focus{index:0,short:false},Node::default(),BackgroundColor(Color::WHITE))).id();
    assert!(app.world().get::<ImageNode>(cursor).is_none());
    let tint=crate::interface::tint(crate::interface::hud_rgb(),255.);
    app.update();
    let image=app.world().get::<ImageNode>(cursor).expect("real instrument producer inserts the loaded cursor on its first frame");
    assert!(app.world().resource::<Assets<Image>>().get(&image.image).is_some());
    assert_eq!(image.color,tint,"insertion keeps the pre-clear selection tint");
    assert_eq!(app.world().get::<BackgroundColor>(cursor).unwrap().0,Color::NONE,"first insertion frame already has no solid backing; a second paint must not be needed");
    app.update();
    assert_eq!(app.world().get::<ImageNode>(cursor).unwrap().color,tint);
    assert_eq!(app.world().get::<BackgroundColor>(cursor).unwrap().0,Color::NONE);
    app.world_mut().resource_mut::<Appearance>().theme=Theme::Minimal;
    app.update();
    assert!(app.world().get::<ImageNode>(cursor).is_none());
    assert_eq!(app.world().get::<BackgroundColor>(cursor).unwrap().0,tint,"removing the installed cursor restores the procedural cue in the same frame");
}

#[test]
fn legacy_theme_names_load_and_save_their_canonical_choices() {
    let mut app=App::new();
    install(&mut app);
    let (load,chosen,save)={
        let settings=app.world().resource::<crate::settings::Settings>();
        let row=settings.index(THEME_KEY).unwrap();
        let choices=settings.choices(row).unwrap();
        assert_eq!((choices.names)(app.world()),["Normal","Classic","Field Notes","Retro Console","Wrist Terminal"]);
        (choices.load,choices.chosen,choices.save)
    };
    for (legacy,value,current) in [("Split Panel",Theme::SplitPanel,"Field Notes"),
        ("Classic Glass",Theme::ClassicGlass,"Classic"),("Minimal",Theme::Minimal,"Normal")] {
        assert!(load(app.world_mut(),legacy));
        assert_eq!(app.world().resource::<Appearance>().theme,value);
        assert_eq!(chosen(app.world()),current);
        assert_eq!(save(app.world()),current);
    }
}
