use bevy::ecs::system::RunSystemOnce;

#[test]
fn actual_settings_note_region_preview_is_live_passive_and_keeps_original_note_and_actions() {
    let mut app=settings_app(&[("difficulty","Gameplay")],Page::Settings{selected:None});
    app.add_plugins(bevy::input::InputPlugin);
    app.init_resource::<crate::dialogue_menu::DialogueMenu>();
    crate::dialogue_touch::install(&mut app);
    let enabled=app.world().resource::<Settings>().index("touch-dialogue").unwrap();
    if !app.world().resource::<crate::dialogue_touch::TouchDialogue>().enabled {
        assert_eq!(crate::settings::flip(app.world_mut(),enabled),Some(true));
    }
    let row=app.world().resource::<Settings>().index(crate::dialogue_touch::appearance::THEME_KEY).unwrap();
    assert!(crate::settings::pick(app.world_mut(),row,"Field Notes"));
    *app.world_mut().resource_mut::<Page>()=Page::Settings{selected:Some(RowId::Setting(row))};
    app.update();app.update();
    let preview=app.world_mut().query_filtered::<Entity,With<crate::dialogue_touch::appearance::Preview>>().single(app.world()).unwrap();
    let mut entities=vec![preview];let mut at=0;
    while at<entities.len() {
        if let Some(children)=app.world().get::<Children>(entities[at]) {entities.extend(children.iter());}
        at+=1;
    }
    assert!(entities.iter().all(|e|app.world().get::<Button>(*e).is_none()&&app.world().get::<PauseButton>(*e).is_none()));
    assert!(entities.iter().all(|e|app.world().get::<bevy::picking::Pickable>(*e)==Some(&bevy::picking::Pickable::IGNORE)));
    assert_eq!(app.world().get::<Node>(preview).unwrap().display,Display::Flex);
    assert!(app.world().get::<Node>(preview).unwrap().overflow==Overflow::clip());
    let notes:Vec<_>=app.world_mut().query_filtered::<&Text,With<SwitchNote>>().iter(app.world()).map(|t|t.0.clone()).collect();
    let registered=app.world().resource::<Settings>().list()[row].note;
    assert!(notes.iter().any(|n|n.contains(registered)));
    let mut before:Vec<_>=app.world_mut().query::<&PauseButton>().iter(app.world()).map(|b|format!("{b:?}")).collect();before.sort();
    assert!(app.world_mut().query::<&PauseButton>().iter(app.world()).any(|b|*b==PauseButton::Row(RowId::Setting(row))));
    assert_eq!(app.world().get::<BackgroundColor>(preview).unwrap().0.alpha(),0.,"preview enclosing surface is intentionally unframed");
    let mut selected=app.world_mut().query_filtered::<&BackgroundColor,With<crate::dialogue_touch::appearance::PreviewSelection>>();
    assert!(selected.single(app.world()).unwrap().0.alpha()>0.,"selected response initially shows configured backing");
    let opacity=app.world().resource::<Settings>().index(crate::dialogue_touch::appearance::BACKGROUND_KEY).unwrap();
    crate::settings::set_slider(app.world_mut(),opacity,0.0);app.update();app.update();
    let mut q=app.world_mut().query_filtered::<&BackgroundColor,With<crate::dialogue_touch::appearance::PreviewSelection>>();
    assert_eq!(q.single(app.world()).unwrap().0.alpha(),0.);
    let mut after:Vec<_>=app.world_mut().query::<&PauseButton>().iter(app.world()).map(|b|format!("{b:?}")).collect();after.sort();
    assert_eq!(after,before);
    for name in ["Normal","Classic","Retro Console","Wrist Terminal","Field Notes"] {
        assert!(crate::settings::pick(app.world_mut(),row,name));
        app.update();app.update();
        let mut samples=app.world_mut().query_filtered::<&Node,With<crate::dialogue_touch::appearance::Preview>>();
        if name=="Field Notes" {
            assert_eq!(samples.single(app.world()).unwrap().display,Display::Flex);
        } else {
            assert!(samples.iter(app.world()).all(|n|n.display==Display::None));
        }
    }
    *app.world_mut().resource_mut::<Page>()=Page::Settings{selected:Some(RowId::Setting(0))};app.update();
    assert_eq!(app.world_mut().query_filtered::<Entity,With<crate::dialogue_touch::appearance::Preview>>().iter(app.world()).count(),0);
}

#[test]
fn appearance_preview_never_replaces_a_narrow_note_or_its_required_reason() {
    let mut app=App::new();
    app.init_resource::<crate::dialogue_menu::DialogueMenu>();crate::dialogue_touch::install(&mut app);
    app.init_resource::<crate::pad::PadGlyphs>();
    let row=app.world().resource::<Settings>().index(crate::dialogue_touch::appearance::HINTS_KEY).unwrap();
    let d=Drawn{page:Page::Settings{selected:Some(RowId::Setting(row))},entries:vec![ViewEntry::Row(RowId::Setting(row))],lines:1,..default()};
    app.world_mut().run_system_once_with(move |In(d):In<Drawn>,mut commands:Commands,settings:Res<Settings>,glyphs:Res<crate::pad::PadGlyphs>| {
        commands.spawn(Node::default()).with_children(|root|settings_page(root,&d,&PauseLook::plain(),settings.list(),Some(RowId::Setting(row)),ListBox{left:0.,width:400.},&glyphs));
    },d).unwrap();
    assert_eq!(app.world_mut().query_filtered::<Entity,With<crate::dialogue_touch::appearance::Preview>>().iter(app.world()).count(),0);
    let notes:Vec<_>=app.world_mut().query_filtered::<&Text,With<SwitchNote>>().iter(app.world()).map(|t|t.0.clone()).collect();
    assert!(notes.iter().any(|n|n.contains("NPC speech, choices, requirements")));
    assert!(app.world_mut().query::<&PauseButton>().iter(app.world()).any(|b|*b==PauseButton::Row(RowId::Setting(row))));
}
#[test]
fn selected_appearance_note_and_real_unavailable_reason_keep_the_visible_uncut_word() {
    #[derive(Resource)] struct RequiredWord(String);
    const WORD:&str=concat!("WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW",
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW",
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW");
    let lb=ListBox{left:0.,width:600.};
    let look=PauseLook::plain();let font=&look.faces.small.font;
    let reduced=note_width(ListBox{width:lb.width-220.,..lb});
    let full=note_width(lb);
    let n=(1..=WORD.len()).find(|&n|font.width(&WORD[..n])>reduced).expect("authored font must measure nonzero word advances");
    let word=&WORD[..n];
    assert!(font.width(word)>reduced && font.width(word)<=full);
    assert!(font.lines(word,Some(reduced)).iter().any(|line|line.width>reduced));
    assert!(note_wrapped(font,word,reduced).iter().any(|line|line.text().ends_with(CUT_MARK)),"old truncated fit input is a real boundary witness");
    for required_reason in [false,true] {
        let mut app=App::new();
        app.init_resource::<crate::dialogue_menu::DialogueMenu>().init_resource::<crate::pad::PadGlyphs>();
        crate::dialogue_touch::appearance::register_theme_note(&mut app,if required_reason {"Actual theme note"}else{word});
        crate::dialogue_touch::install(&mut app);
        let key = "preview-extra-a";
        add_setting(&mut app,Setting{key,group:GROUP_MENUS,label:key,note:"",switches:"",get:|_|false,set:|_,_|{},slider:None});
        app.insert_resource(RequiredWord(word.into()));
        if required_reason {crate::settings::set_choice_unavailable(&mut app,crate::dialogue_touch::appearance::THEME_KEY,|w,_|Some(w.resource::<RequiredWord>().0.clone()));}
        app.world_mut().resource_mut::<crate::dialogue_touch::appearance::Appearance>().theme=crate::dialogue_touch::appearance::Theme::SplitPanel;
        let settings=app.world().resource::<Settings>();let row=settings.index(crate::dialogue_touch::appearance::THEME_KEY).unwrap();
        let choices=settings.choices(row).expect("actual theme ChoiceList must be installed, not an ordinary same-key row");let names=(choices.names)(app.world());let chosen=(choices.chosen)(app.world());
        assert_eq!(names,["Normal","Classic","Field Notes","Retro Console","Wrist Terminal"]);
        assert_eq!(chosen,"Field Notes");
        let looks=ListLooks{labels:names.clone(),reasons:names.iter().map(|name|settings.choice_unavailable(app.world(),row,name)).collect(),chosen:(chosen.clone(),settings.choice_unavailable(app.world(),row,&chosen))};
        let mut entries:Vec<_>=(0..settings.list().len()).filter(|i|*i!=row).map(|i|ViewEntry::Row(RowId::Setting(i))).collect();entries.push(ViewEntry::Row(RowId::Setting(row)));
        assert_eq!(entries.len(),12);
        let d=Drawn{page:Page::Settings{selected:Some(RowId::Setting(row))},entries,lines:12,lists:vec![(row,names,chosen)],looks:vec![(row,looks)],dropdown:required_reason.then_some(Dropdown{row,at:0,first:0}),..default()};
        let note_top=OPTIONS_TOP+12.*OPTION_ROW+NOTE_GAP;
        assert!(note_bottom()-note_top>=crate::dialogue_touch::appearance::MIN_PREVIEW_UNITS);
        assert_eq!(note_fit(font.lines(word,Some(full)).len(),note_top),1);
        if required_reason {
            assert_eq!(d.choice_reason(row,0),Some(word),"real registered unavailable callback supplies the reason");
            let row_top=OPTIONS_TOP+11.*OPTION_ROW;let popup_h=5.*DROP_ITEM+2.*LINE;
            assert!(row_top+OPTION_ROW+popup_h>950. && note_top>row_top,"real dropdown opens above, clear of the note witness");
        }
        app.world_mut().run_system_once_with(move |In(d):In<Drawn>,mut commands:Commands,settings:Res<Settings>,glyphs:Res<crate::pad::PadGlyphs>| {
            commands.spawn(Node::default()).with_children(|root|settings_page(root,&d,&PauseLook::plain(),settings.list(),Some(RowId::Setting(row)),lb,&glyphs));
        },d).unwrap();
        assert_eq!(app.world_mut().query_filtered::<Entity,With<crate::dialogue_touch::appearance::Preview>>().iter(app.world()).count(),0);
        let visible:Vec<_>=app.world_mut().query_filtered::<(Entity,&Text),Without<SwitchNote>>().iter(app.world()).filter(|(_,t)|t.0==word).map(|(e,_)|e).collect();
        assert_eq!(visible.len(),1,"the actual plain-font note painter emits the whole word, not the hidden SwitchNote copy");
        let mut at=Some(visible[0]);
        while let Some(entity)=at {
            assert!(app.world().get::<Node>(entity).is_none_or(|node|node.display!=Display::None));
            assert!(app.world().get::<bevy::prelude::Visibility>(entity).is_none_or(|visibility|*visibility!=bevy::prelude::Visibility::Hidden));
            at=app.world().get::<ChildOf>(entity).map(ChildOf::parent);
        }
    }
}
