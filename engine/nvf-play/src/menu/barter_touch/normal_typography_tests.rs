// Actual ordinary painters, measured layout and raw menu effects. Source-only / UNRUN.
// Fonts deliberately use the production missing-font fallback; marker DDS bytes are authored.
struct TypographyFiles(std::path::PathBuf);
impl Drop for TypographyFiles {
    fn drop(&mut self) { let _=std::fs::remove_dir_all(&self.0); }
}
fn typography_data()->(TypographyFiles,nvf_assets::GameData) {
    let root=std::env::temp_dir().join(format!("nvf-menu-typography-{}-{}",std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let mut dds=crate::interface::tests::authored_rgba_dds(2,2);
    for pixel in dds[128..].as_chunks_mut::<4>().0 {pixel.copy_from_slice(&[255;4]);}
    for file in ["textures/square.dds","textures/square_filled.dds",
        "textures/interface/shared/marker/square.dds","textures/interface/shared/marker/square_filled.dds"] {
        let path=root.join(file);std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path,&dds).unwrap();
    }
    let vfs=fnv_vfs::Vfs::open(&root,&[]).unwrap();
    let schema=fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
    (TypographyFiles(root),nvf_assets::GameData::new(std::sync::Arc::new(vfs),std::sync::Arc::new(schema)))
}
fn typography_scene(native:bool,size:f32,long:bool)->(App,FormId,TypographyFiles) {
    let pair=if long {normal_long_simulation()} else {simulation()};
    typography_scene_with_sim(native,size,pair)
}
fn typography_marker_scene(size:f32)->(App,FormId,TypographyFiles) {
    let (mut sim,stock)=simulation();
    // Real fixture input before open_barter: keep the player's coins, but avoid
    // unoffered player stacks interleaving the two actual bought/staged rows.
    for i in 0..10 {
        assert_eq!(sim.world.inventory_mut(PLAYER).unwrap().remove(FormId(GOODS.0+i),8),8);
    }
    typography_scene_with_sim(false,size,(sim,stock))
}
fn typography_scene_with_sim(native:bool,size:f32,pair:(crate::sim::SimState,FormId))->(App,FormId,TypographyFiles) {
    let (files,data)=typography_data();
    let (app,stock)=normal_scene_with_setup(native,pair,move |app| {
        app.insert_resource(data).insert_resource(crate::settings::SettingsFile(None));
        crate::menu_readability::register(app,crate::pause::GROUP_MENUS);
        let index=app.world().resource::<crate::settings::Settings>().index("barter-menu-size-v1").unwrap();
        let part = crate::settings::choice_part(((size - 0.5) / 0.05).round() as usize, 31);
        assert_eq!(crate::settings::set_slider(app.world_mut(),index,part),Some(part));
        assert_eq!(app.world().resource::<crate::menu_readability::MenuReadability>()
            .size(crate::menu_readability::MenuKind::Barter),size);
        if native {crate::tiles::draw::install_typography_fixture_painter(app);}
    });
    (app,stock,files)
}
fn typography_rect(world:&bevy::prelude::World,e:Entity)->Rect {
    let node=world.get::<ComputedNode>(e).expect("actual Taffy node");
    assert!(node.size().is_finite() && node.size().min_element()>=0.0);
    Rect::from_center_size(world.get::<bevy::ui::UiGlobalTransform>(e).unwrap().translation,node.size())
}
fn typography_descendants(world:&bevy::prelude::World,root:Entity)->Vec<Entity> {
    let mut out=vec![root];let mut i=0;
    while i<out.len() {let e=out[i];i+=1;
        if let Some(kids)=world.get::<Children>(e){out.extend(kids.iter());}
        assert!(out.len()<4096,"bounded authored painter hierarchy");
    }
    out
}
fn typography_row(app:&mut App,side:Side,index:usize)->Entity {
    let mut rows=app.world_mut().query::<(Entity,&crate::menu::RowButton)>();
    rows.iter(app.world()).find_map(|(e,r)|(r.0==side && r.1==index).then_some(e)).expect("actual ordinary row button")
}
fn typography_text(world:&bevy::prelude::World,row:Entity,wanted:&str)->Entity {
    typography_descendants(world,row).into_iter().find(|&e|
        world.get::<Text>(e).is_some_and(|s|s.0==wanted)).expect("actual painted row text")
}
fn typography_font_pixels(world:&bevy::prelude::World,e:Entity,height:f32)->f32 {
    match &world.get::<TextFont>(e).unwrap().font_size {
        FontSize::Vh(v)=>*v*height/100.0,
        other=>panic!("real interface painter must retain viewport font units: {other:?}"),
    }
}
fn typography_baseline(world:&bevy::prelude::World,e:Entity,height:f32)->f32 {
    let anchor=world.get::<ChildOf>(e).unwrap().parent();
    typography_rect(world,anchor).min.y+typography_font_pixels(world,e,height)
}
fn typography_clip(world:&bevy::prelude::World,e:Entity,window:Rect)->Option<(Rect,Rect)> {
    let mut clip=window;
    let actual=world.get::<bevy::ui::CalculatedClip>(e)?;
    for part in actual.rects()? {
        let to_world=part.world_to_clip_local.inverse();
        let a=to_world.transform_point2(part.rect.min);let b=to_world.transform_point2(part.rect.max);
        clip=clip.intersect(Rect::from_corners(a.min(b),a.max(b)));
    }
    let visible=typography_rect(world,e).intersect(clip);
    (visible.width()>0.0 && visible.height()>0.0).then_some((visible,clip))
}
fn typography_row_metrics(app:&mut App,index:usize)->(Rect,f32,f32,f32) {
    let row=typography_row(app,Side::Container,index);
    let source=&model(app).list(Side::Container).rows[index];
    let label=crate::menu::look::row_text(source);
    let price=source.price.map_or("--".to_string(),crate::menu::barter::price_text);
    let world=app.world();let name=typography_text(world,row,&label);let cost=typography_text(world,row,&price);
    let f=world.resource::<Fixture>().0;let height=f.size.y*f.density;
    (typography_rect(world,row),typography_baseline(world,name,height),
        typography_baseline(world,cost,height),typography_font_pixels(world,name,height))
}

#[test]
fn normal_barter_actual_hb_name_and_price_share_scaled_baseline_and_default_geometry() {
    let (mut old,_)=normal_scene(false);
    let index=model(&old).list(Side::Container).rows.iter().position(|r|r.name.starts_with("Supply")).unwrap();
    let original=typography_row_metrics(&mut old,index);
    for size in [0.5,1.0,1.5] {
        let (mut app,_,_files)=typography_scene(false,size,false);
        let (rect,name,cost,font)=typography_row_metrics(&mut app,index);
        assert!((name-cost).abs()<=0.6,"actual painted first baselines must agree at {size}");
        let physical_unit=app.world().resource::<Fixture>().0.size.y/960.0*crate::interface::menu_scale();
        assert!((name-rect.min.y-35.0*size*physical_unit).abs()<=1.1,"baseline follows measured typography, not an unscaled price top");
        assert!((font-original.3*size).abs()<=0.01,"actual TextFont uses the saved factor");
        assert!(rect.height()+1.1>=51.0*physical_unit,"smaller letters retain the original one-line hit row");
        if size==1.0 {
            assert!(rect.min.abs_diff_eq(original.0.min,0.01) && rect.max.abs_diff_eq(original.0.max,0.01));
            assert!((name-original.1).abs()<=0.01 && (cost-original.2).abs()<=0.01,"100% retains old actual drawing geometry");
        }
    }
}

#[test]
fn normal_barter_actual_adjacent_staged_markers_stay_inside_rows_at_all_saved_sizes() {
    for size in [0.5,1.0,1.5] {
        let (mut app,stock,_files)=typography_marker_scene(size);
        let goods:Vec<_>=model(&app).list(Side::Container).rows.iter().filter(|r|r.name.starts_with("Supply"))
            .take(2).map(|r|r.item).collect();assert_eq!(goods.len(),2);
        for item in goods.iter().copied() {
            let index=model(&app).list(Side::Container).rows.iter().position(|r|r.item==item).unwrap();
            let p=normal_point(&mut app,|t|t==normal::Target::Row(Side::Container,index));
            normal_tap_at(&mut app,p);assert!(model(&app).quantity.is_some());
            for _ in 0..7 {normal_command(&mut app,Command::AmountStep(-1));}
            assert_eq!(model(&app).quantity.unwrap().amount,1);
            normal_command(&mut app,Command::AmountOk);
        }
        assert_eq!(offered(&app),2,"only the real quantity consumer staged two units");
        let marked:Vec<_>=model(&app).list(Side::Player).rows.iter().enumerate().filter_map(|(i,r)|r.bartered.then_some(i)).collect();
        assert_eq!(marked.len(),2);assert_eq!(marked[1],marked[0]+1,"the actual staged rows are adjacent");
        let mut prior=None;
        for index in marked {
            let e=typography_row(&mut app,Side::Player,index);let r=typography_rect(app.world(),e);
            let markers:Vec<_>=typography_descendants(app.world(),e).into_iter().filter(|&e|app.world().get::<ImageNode>(e).is_some()).collect();
            assert_eq!(markers.len(),1,"authored square DDS is loaded by the actual row painter");
            let image=app.world().get::<ImageNode>(markers[0]).unwrap();
            assert!(app.world().resource::<Assets<Image>>().get(&image.image).is_some());
            let mark=typography_rect(app.world(),markers[0]);
            assert!(mark.min.x>=r.min.x-1.1 && mark.min.y>=r.min.y-1.1 && mark.max.x<=r.max.x+1.1 && mark.max.y<=r.max.y+1.1);
            if let Some(previous)=prior {assert!(mark.min.y>=previous,"neighboring actual row markers cannot overlap");}
            prior=Some(mark.max.y);
        }
        assert_eq!(count(&mut app,PLAYER,CAPS),1000);assert_eq!(count(&mut app,stock,CAPS),1000,"staging is not payment");
        for item in goods {
            assert_eq!(count(&mut app,PLAYER,item),0,"staged Player presentation is not delivery");
            assert_eq!(count(&mut app,stock,item),8,"merchant retains goods until Accept");
        }
    }
}

#[test]
fn normal_barter_actual_native_and_hb_typography_paint_clip_and_drag_keep_transaction_state() {
    for native in [false,true] {
        let mut fixed_clip:Option<Rect>=None;
        for size in [0.5,1.0,1.5] {
            let (mut long,_,_files)=typography_scene(native,size,true);
            let mut texts=long.world_mut().query::<(Entity,&Text)>();
            let labels:Vec<_>=texts.iter(long.world()).filter_map(|(e,t)|t.0.starts_with("Supply").then_some(e)).collect();
            assert!(!labels.is_empty(),"real native/HB painter emitted measured wrapped labels");
            let f=long.world().resource::<Fixture>().0;
            let window=Rect::from_corners(Vec2::ZERO,f.size*f.density);
            let visible:Vec<_>=labels.iter().filter_map(|&e|typography_clip(long.world(),e,window)).collect();
            assert!(!visible.is_empty(),"actual clipped wrapped labels have nonempty screen intersection");
            let clip=visible[0].1;
            assert!(clip.width()>0.0 && clip.height()>0.0);
            if let Some(old)=fixed_clip {assert!(clip.min.abs_diff_eq(old.min,1.1) && clip.max.abs_diff_eq(old.max,1.1),
                "typography changes wrapping, not the actual native/HB list viewport");}
            else {fixed_clip=Some(clip);}
            for (painted,bounds) in &visible {
                assert!(painted.min.x>=bounds.min.x && painted.min.y>=bounds.min.y && painted.max.x<=bounds.max.x && painted.max.y<=bounds.max.y);
            }
            for e in labels {
                let actual=typography_font_pixels(long.world(),e,f.size.y*f.density);
                let expected=31.0*0.75*size*crate::interface::menu_scale()*f.size.y/960.0;
                assert!((actual-expected).abs()<=0.01,"writer and actual glyph/Text painter share the saved factor");
                assert!(long.world().get::<bevy::ui::CalculatedClip>(e).is_some(),"actual wrapped text inherits native/HB list clipping");
            }
            let (mut app,stock,_short_files)=typography_scene(native,size,false);
            let selected=model(&app).selected;let focus=model(&app).focus;
            let before=normal_scroll_value(&app,native,Side::Container);
            let p=normal_point(&mut app,|t|matches!(t,normal::Target::Row(Side::Container,_)));
            edge(&mut app,310,TouchPhase::Started,p);app.update();
            edge(&mut app,310,TouchPhase::Moved,p-Vec2::Y*48.0);app.update();
            edge(&mut app,310,TouchPhase::Ended,p-Vec2::Y*48.0);app.update();
            assert!(normal_scroll_value(&app,native,Side::Container)>before,"real current measured list consumes raw drag");
            assert_eq!(model(&app).selected,selected);assert_eq!(model(&app).focus,focus);
            assert_eq!(offered(&app),0);assert!(model(&app).quantity.is_none());
            assert_eq!(count(&mut app,PLAYER,CAPS),1000);assert_eq!(count(&mut app,stock,CAPS),1000);
        }
    }
}
