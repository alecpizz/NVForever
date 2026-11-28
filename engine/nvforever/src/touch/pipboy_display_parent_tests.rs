// Registered settings + existing sole reader + real Taffy display/native menu.
// These controls deliberately start away from the old edge cue rectangles.
fn display_choice(app:&mut App,name:&str) {
    let row=app.world().resource::<nvf_play::settings::Settings>().index("touch-pipboy-dismiss-side-v2").unwrap();
    let choice=app.world().resource::<nvf_play::settings::Settings>().choices(row).unwrap();
    if (choice.chosen)(app.world())!=name {assert!(nvf_play::settings::pick(app.world_mut(),row,name));}
    assert_eq!((choice.chosen)(app.world()),name);
    for _ in 0..4 {app.update();}
}
fn display_side_point(app:&mut App,left:bool)->Vec2 {
    let flat=diagonal_flat(app);
    let (safe,size)={let world=app.world_mut();let mut q=world.query_filtered::<&Window,With<PrimaryWindow>>();
        (*world.resource::<nvf_play::touch_safe_area::SafeInsets>(),q.single(world).unwrap().resolution.size())};
    let x=if left {(safe.left+flat.min.x)*0.5} else {(flat.max.x+size.x-safe.right)*0.5};
    // Near the display's top, away from the vertically centered tiny cue.
    let p=Vec2::new(x,flat.min.y+8.0);
    assert!(!flat.contains(p));
    assert!(p.x>=safe.left && p.x<=size.x-safe.right);
    p
}
fn display_outside(app:&mut App,left:bool)->Vec2 {
    let p=display_side_point(app,left);
    assert!(!diagonal_painted(app,left).contains(p),"the whole outside-display area, not only the old cue, must work");
    p
}
fn display_down(app:&mut App,w:Entity,id:u64,p:Vec2,left:bool,terminal:TouchPhase) {
    let safe=app.world().resource::<nvf_play::touch_safe_area::SafeInsets>().bottom;
    let size=app.world().get::<Window>(w).unwrap().resolution.size();
    let travel=120.0_f32.min(size.y-safe-p.y);
    let end=p+Vec2::new(if left {-8.} else {8.},travel*0.9);
    touch(app,w,id,TouchPhase::Started,p.x,p.y);app.update();
    touch(app,w,id,TouchPhase::Moved,end.x,end.y);app.update();
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),"movement is not a flat-menu Close");
    touch(app,w,id,terminal,end.x,end.y);for _ in 0..6 {app.update();}
}
#[test]
fn actual_flat_outside_display_both_sides_work_beyond_cues_at_safe_phone_scales_and_hidden_graphics() {
    for (size,density,scale) in [(Vec2::new(400.,300.),1.,0.75),(Vec2::new(640.,360.),2.,1.5),(Vec2::new(792.,363.),2.,1.)] {
        for left in [true,false] {for graphics in [false,true] {
            let (mut app,w)=diagonal_flat_scene(size,density,scale,graphics);
            display_choice(&mut app,"Both");let p=display_outside(&mut app,left);
            display_down(&mut app,w,1700,p,left,TouchPhase::Ended);
            assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
            let seen=app.world().resource::<DefaultMenuTouchSeen>();
            assert_eq!((seen.presses,seen.releases,seen.scrolls),(0,0,0),"owned End cannot select or scroll beneath the dismissal");
        }}
    }
}
#[test]
fn actual_flat_chooser_rejects_disabled_and_opposite_side_while_display_scroll_stays_live() {
    for (name,left,accept) in [("Disabled",true,false),("Disabled",false,false),("Left only",false,false),("Right only",true,false),("Left only",true,true),("Right only",false,true)] {
        let (mut app,w)=diagonal_flat_scene(Vec2::new(640.,360.),1.,1.,true);
        display_choice(&mut app,name);
        let p=display_side_point(&mut app,left);
        assert!(!diagonal_flat(&mut app).contains(p),"opposite/Disabled control must remain outside the CURRENT crop after mode changes");
        display_down(&mut app,w,1710,p,left,TouchPhase::Ended);
        assert_eq!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none(),accept);
    }
    let (mut app,w)=diagonal_flat_scene(Vec2::new(640.,360.),1.,1.,false);display_choice(&mut app,"Both");
    let p=diagonal_flat(&mut app).center();
    for (phase,at) in [(TouchPhase::Started,p),(TouchPhase::Moved,p+Vec2::Y*70.),(TouchPhase::Ended,p+Vec2::Y*70.)] {touch(&mut app,w,1711,phase,at.x,at.y);app.update();}
    for _ in 0..6 {app.update();}
    assert!(app.world().resource::<DefaultMenuTouchSeen>().scrolls>0);
    assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
}
#[test]
fn actual_flat_outside_display_reverse_cancel_and_late_side_change_retire_owned_end_then_recover() {
    for cause in ["reverse","cancel","mode","horizontal"] {
        let (mut app,w)=diagonal_flat_scene(Vec2::new(640.,360.),1.,1.,true);let p=display_outside(&mut app,false);
        touch(&mut app,w,1720,TouchPhase::Started,p.x,p.y);app.update();
        if cause=="horizontal" {touch(&mut app,w,1720,TouchPhase::Moved,p.x-60.,p.y);app.update();}
        touch(&mut app,w,1720,TouchPhase::Moved,p.x-40.,p.y+100.);app.update();
        if cause=="mode" {display_choice(&mut app,"Disabled");}
        let end=p+Vec2::new(-40.,if cause=="reverse" {12.} else {100.});
        touch(&mut app,w,1720,if cause=="cancel" {TouchPhase::Canceled}else{TouchPhase::Ended},end.x,end.y);
        for _ in 0..6 {app.update();}
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some());
        assert_eq!(app.world().resource::<DefaultMenuTouchSeen>().presses,0);
        display_choice(&mut app,"Both");let p=display_outside(&mut app,true);
        display_down(&mut app,w,1721,p,true,TouchPhase::Ended);
        assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
    }
}

#[test]
fn actual_fallback_opening_keeps_vertical_winner_through_up_down_up_with_persistent_side_drift() {
    for horizontal_first in [false,true] {
        let (mut app,w)=fallback_native_scene(false);let p=fallback_band_point(&mut app,w);
        let dx=if p.x<320. {40.} else {-40.};
        touch(&mut app,w,1730,TouchPhase::Started,p.x,p.y);app.update();
        if horizontal_first {touch(&mut app,w,1730,TouchPhase::Moved,p.x+dx,p.y);app.update();}
        for dy in [-100.,-20.,-120.] {
            touch(&mut app,w,1730,TouchPhase::Moved,p.x+dx,p.y+dy);app.update();
            assert!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_none());
        }
        touch(&mut app,w,1730,TouchPhase::Ended,p.x+dx,p.y-120.);app.update();app.update();
        assert_eq!(app.world().resource::<nvf_play::menu::ActiveMenu>().0.is_some(),!horizontal_first,
            "vertical-first remains owned through reversal; horizontal-first stays canceled");
    }
}
