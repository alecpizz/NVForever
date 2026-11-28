//! Supplied Classic hardware around live dialogue, never captured scene or text.
use super::*;
use super::classic::{Cache, CapsText, Layout};
use crate::{interface::Face, touch_safe_area::SafeInsets};
use nvf_assets::GameData;

/// Prepared asset/font/layout input. The camera lease separately admits its actual
/// world target; this resource alone never means a frame was rendered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Prepared {
    pub window: Entity,
    pub size: Vec2,
    pub density: f32,
    pub ui_scale: f32,
    pub font:bevy::asset::AssetId<Image>,
    pub casing:bevy::asset::AssetId<Image>,
    pub origin: Vec2,
    pub canvas_size: Vec2,
    pub layout: Layout,
    caps: CapsText,
}
#[derive(Resource, Default)]
pub(crate) struct Surface(pub Option<Prepared>);
#[derive(Resource, Default)]
struct FontCache {faces:Option<crate::terminal::screen::Faces>,warned:bool,geometry_warned:bool}
#[derive(Component)]
struct ClassicRoot;

pub(super) fn install(app: &mut App) {
    classic::install(app);
    app.init_resource::<Surface>().init_resource::<FontCache>()
        .add_systems(Update, prepare.after(crate::PlaySet::Scripts)
            .before(crate::dialogue_split::Resolve).before(crate::PlaySet::Ui))
        .add_systems(Update, draw.in_set(crate::PlaySet::Ui)
            .after(crate::dialogue_menu::DrawMenu).before(super::draw));
}

/// The counter's metrics for `text`, per unit of glyph size. The layout sizes the
/// glyphs from the counter opening's height and widens the opening to the value.
fn caps_fit(face: &Face, text: &str) -> Option<CapsText> {
    let size = face.font.size;
    if !size.is_finite() || size <= 0.0 { return None; }
    let (mut advance, mut left, mut right, mut descent) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    let baseline = crate::interface::line_base(face);
    for &c in text.as_bytes() {
        let glyph = face.font.glyph(c);
        let step = glyph.advance();
        if !glyph.drawn() || !step.is_finite() || step <= 0.0 { return None; }
        advance += step;
        // The softened atlas extends beyond the original advance at both ends.
        left = left.max(-glyph.before);
        right = right.max(-glyph.after);
        descent = descent.max(glyph.height - glyph.ascent);
    }
    let fit = CapsText { left: left / size, width: (left + advance + right) / size,
        height: (baseline + descent).max(size * 1.2) / size };
    (fit.width.is_finite() && fit.width > 0.0 && fit.height.is_finite()).then_some(fit)
}

fn geometry_with_budget(template:classic::Template,size:Vec2,safe:SafeInsets,caps:CapsText)->Option<(Vec2,Layout)> {
    let safe=SafeInsets::from_array([safe.left,safe.top,safe.right,safe.bottom])?;
    let origin=Vec2::new(safe.left,safe.top);
    Some((origin,Layout::new(template,size-origin-Vec2::new(safe.right,safe.bottom),
        [safe.left,safe.top,safe.right,safe.bottom],caps)?))
}

/// This is the one effective-presentation predicate used by the parent input,
/// native fallback and capture admission. Prepared assets alone are insufficient.
pub(super) fn ready_for(window:Entity,surface:Option<&Surface>,frame:&crate::dialogue_split::Frame)->bool {
    surface.and_then(|s|s.0.as_ref()).is_some_and(|p| {
        let world=Rect::from_corners(p.layout.world.min+p.origin,p.layout.world.max+p.origin);
        let min=(world.min*p.density).floor();let max=(world.max*p.density).ceil();
        p.window==window && frame.ui_camera.is_some() && frame.classic.is_some_and(|v|
            v.prepared==*p && v.window==window && v.world==world && v.window_size==p.size
                && v.density==p.density && v.ui_scale==p.ui_scale
                && v.physical_position==min.as_uvec2() && v.physical_size==(max-min).as_uvec2())
    })
}

type AssetInputs<'w>=(Option<Res<'w,GameData>>,Option<ResMut<'w,Assets<Image>>>,
    Option<Res<'w,bevy::image::CompressedImageFormatSupport>>);
#[allow(clippy::too_many_arguments)]
fn prepare(menu: Res<DialogueMenu>, settings: Res<TouchDialogue>, appearance: Res<appearance::Appearance>,
    safe: Res<SafeInsets>, gestures: Res<Gestures>, windows: Query<(Entity,&Window),With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>, (data,images,formats): AssetInputs<'_>,
    (state,paused,script,creator): Availability<'_>, mut cache: ResMut<Cache>,
    mut fonts: ResMut<FontCache>, mut surface: ResMut<Surface>, counter: Res<classic_state::State>) {
    let next=(|| {
        if appearance.theme!=appearance::Theme::ClassicGlass || !settings.enabled
            || !gestures.focused || !gestures.running
            || !available(state.as_deref(),paused.as_deref(),script.as_deref(),creator.as_deref()) {return None;}
        let (entity,window)=windows.single().ok()?;
        let density=window.scale_factor();
        let ui_scale=scale.as_ref().map_or(1.0,|s|s.0);
        if !window.focused || !density.is_finite() || density<=0.0
            || !ui_scale.is_finite() || ui_scale<=0.0 {return None;}
        menu.view.as_ref()?;
        let mut images=images?;
        cache.prepare(data.as_deref(),&mut images);
        let art=cache.loaded()?;
        let casing=art.image_id();
        let data=data.as_deref()?;
        let faces=fonts.faces.get_or_insert_with(||crate::terminal::screen::load_faces(Some(data),&mut images,
            crate::interface::bc_supported(formats.as_deref())));
        // A default-font stand-in is not the supplied-art Classic presentation.
        if faces.text.image.is_none() {
            if !fonts.warned {warn!("Classic dialogue needs the installed Font 5 image; using the normal dialogue view");fonts.warned=true;}
            return None;
        }
        let size=Vec2::new(window.width(),window.height());
        // The counter is laid out for the value it shows (0 until one is known).
        let caps = caps_fit(&faces.text, &counter.caps.unwrap_or(0).to_string())?;
        let font=faces.text.image.as_ref()?.0.id();
        let Some((origin,layout))=geometry_with_budget(art.template,size,*safe,caps) else {
            if !fonts.geometry_warned {warn!("Classic dialogue has no usable safe layout; using the normal dialogue view");fonts.geometry_warned=true;}
            return None;
        };
        Some(Prepared {window:entity,size,density,ui_scale,font,casing,origin,
            canvas_size:size-origin-Vec2::new(safe.right,safe.bottom),layout,caps})
    })();
    if surface.0!=next {surface.0=next;}
}

#[derive(Clone,PartialEq)]
struct Key {surface:Prepared,camera:Entity,view:crate::dialogue_menu::MenuView,
    list:Option<ListId>,selected:usize,settings:TouchDialogue,armed:bool,
    spoken:Option<String>,speaking:bool,caps:Option<i32>,barter:Option<classic_state::BarterTarget>}
#[allow(clippy::too_many_arguments)]
fn draw(mut commands:Commands, prepared_surface:Res<Surface>, frame:Res<crate::dialogue_split::Frame>,
    menu:Res<DialogueMenu>, settings:Res<TouchDialogue>, gestures:Res<Gestures>, cache:Res<Cache>, fonts:Res<FontCache>,
    state:Res<classic_state::State>,
    roots:Query<Entity,With<ClassicRoot>>, mut shown:Local<Option<Key>>, mut reveal:ResMut<RevealRequest>) {
    let key=prepared_surface.0.as_ref().and_then(|prepared| {
        let surface=prepared;
        if !ready_for(surface.window,Some(&*prepared_surface),&frame) {return None;}
        Some(Key {surface:*surface,camera:frame.ui_camera?,view:menu.view.clone()?,
            list:menu.list.clone(),selected:menu.selected,settings:*settings,
            armed:gestures.armed==Some(menu.selected),spoken:state.spoken.clone(),speaking:state.speaking,
            caps:state.caps,barter:state.barter.clone()})
    });
    if *shown==key {return;}
    for root in &roots {commands.entity(root).despawn();}
    if let (Some(key),Some(art),Some(faces))=(key.as_ref(),cache.loaded(),fonts.faces.as_ref()) {
        spawn(&mut commands,key,art,&faces.text);
        reveal.0=true;
    }
    *shown=key;
}

fn node(rect:Rect,scale:f32)->Node {
    Node {position_type:PositionType::Absolute,left:px(rect.min.x/scale),top:px(rect.min.y/scale),
        width:px(rect.width()/scale),height:px(rect.height()/scale),..default()}
}
fn label(parent:&mut ChildSpawnerCommands,key:&Key,text:&str,size:f32,width:f32,face:&Face,color:Color) {
    device_canvas::label_on(parent,text,face,size,color,device_canvas::LabelViewport {
        width, ui_scale:key.surface.ui_scale, height:key.surface.size.y,
    });
}
fn spawn(commands:&mut Commands,key:&Key,art:&classic::LoadedArt,face:&Face) {
    let surface=&key.surface;let layout=&surface.layout;let scale=surface.ui_scale;
    // The root sits at the safe canvas. Its casing crops bleed past it to the
    // window edges; text and targets stay inside. Hardware children are passive
    // and do not add touch targets over the upper live world opening.
    let root_node=node(Rect::from_corners(surface.origin,surface.origin+surface.canvas_size),scale);
    commands.spawn((ClassicRoot,PaletteRoot,Pickable::IGNORE,GlobalZIndex(2400),UiTargetCamera(key.camera),root_node))
        .with_children(|root| {
            for rect in layout.wells {
                root.spawn((Pickable::IGNORE,node(rect,scale),BackgroundColor(crate::terminal::screen::screen_color())));
            }
            // Real supplied housing is opaque, independent of the legacy frame slider.
            art.spawn(root,layout,scale,1.0);
            speech(root,key,face);
            responses(root,key,face);
            if let Some(caps)=key.caps {
                let text=caps.to_string();
                // Glyphs follow the counter opening's height; the layout widened the
                // opening for this value, so nothing is shrunk or truncated.
                root.spawn((Pickable::IGNORE,node(layout.caps,scale))).with_children(|p| {
                    let units = 960.0 / surface.size.y;
                    let face = native::scaled_face(face, layout.caps_size * units / face.font.size);
                    let line = &face.font.lines(&text, None)[0];
                    let _units = crate::interface::original_units();
                    crate::interface::set_line(p, &face, line, surface.caps.left(layout.caps_size) * units,
                        crate::interface::line_base(&face), crate::terminal::screen::bright());
                });
            }
            if let Some(target)=&key.barter {
                root.spawn((Button,native_barter(Some(target.list.clone())),node(layout.trade,scale),
                    Name::new("Classic Trade button")));
            } else {
                root.spawn((Pickable::IGNORE,node(layout.red_button,scale),
                    BackgroundColor(Color::BLACK.with_alpha(0.65)),Name::new("Trade unavailable")));
            }

        });
}
fn speech(root:&mut ChildSpawnerCommands,key:&Key,face:&Face) {
    let rect=key.surface.layout.speech;let scale=key.surface.ui_scale;
    let factor=TouchDialogue::theme_text_factor(appearance::Theme::ClassicGlass);
    let mut n=node(rect,scale);n.overflow=Overflow::scroll_y();n.flex_direction=FlexDirection::Column;
    root.spawn((Viewport,ScrollPosition::default(),n)).with_children(|p| {
        let content=Node {width:percent(100.0),min_height:px(48.0/scale),flex_shrink:0.0,flex_direction:FlexDirection::Column,..default()};
        let mut child=p.spawn((Pickable::IGNORE,content));
        if key.speaking {child.insert((Button,native_skip(key.list.clone())));}
        child.with_children(|p| {
            if !key.view.speaker.trim().is_empty() {
                label(p,key,&key.view.speaker,18.0*factor,rect.width(),face,crate::terminal::screen::bright());
            }
            if let Some(line)=&key.spoken {
                label(p,key,line,key.settings.subtitle_size_for(appearance::Theme::ClassicGlass),rect.width(),face,crate::terminal::screen::bright());
            }
        });
    });
}

fn responses(root:&mut ChildSpawnerCommands,key:&Key,face:&Face) {
    let rect=key.surface.layout.responses;let scale=key.surface.ui_scale;
    let factor=TouchDialogue::theme_text_factor(appearance::Theme::ClassicGlass);
    let mut n=node(rect,scale);n.overflow=Overflow::clip();n.flex_direction=FlexDirection::Column;
    root.spawn((Pickable::IGNORE,n)).with_children(|p| {
        if key.view.choices.is_empty() {
            if !key.speaking {return;}
            p.spawn((Button,native_skip(key.list.clone()),Node {width:percent(100.0),min_height:px(48.0/scale),
                padding:UiRect::all(px(6.0/scale)),flex_shrink:0.0,..default()})).with_children(|p|
                label(p,key,"Continue",18.0*factor,rect.width()-12.0,face,crate::terminal::screen::bright()));
            return;
        }
        p.spawn((Viewport,ScrollPosition::default(),Node {width:percent(100.0),flex_grow:1.0,min_height:px(0.0),
            overflow:Overflow::scroll_y(),flex_direction:FlexDirection::Column,..default()})).with_children(|p| {
            for (index,row) in key.view.choices.iter().enumerate() {
                let selected=index==key.selected;
                let ink=if row.short {appearance::check_line()}
                    else if selected {Color::srgb(0.95,0.9,0.2)}
                    else if row.darkened {crate::terminal::screen::read_green()}
                    else {crate::terminal::screen::bright()};
                p.spawn((Button,native_choice(index,key.list.clone(),row.short),Node {width:percent(100.0),
                    min_height:px(48.0/scale),padding:UiRect::all(px(6.0/scale)),flex_shrink:0.0,..default()},
                    Outline::new(px(1.0/scale),px(-1.0/scale),if selected {ink}else{Color::NONE})))
                    .with_children(|p|label(p,key,&row.label,key.settings.choice_size_for(appearance::Theme::ClassicGlass),
                        rect.width()-12.0,face,ink));
            }
        });
        if key.settings.confirm {
            p.spawn((Button,native_confirm(key.list.clone()),Node {width:percent(100.0),min_height:px(48.0/scale),
                padding:UiRect::all(px(6.0/scale)),flex_shrink:0.0,..default()})).with_children(|p|
                label(p,key,if key.armed {"Confirm selection"}else{"Select a choice first"},18.0*factor,
                    rect.width()-12.0,face,crate::terminal::screen::bright()));
        }
    });
}


#[cfg(test)]
pub(crate) use tests::with_barter_surface;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_touch::compact_tests::{layout_scene,rows,send,settle};
    use crate::dialogue_split::{Options,Side};
    use bevy::camera::{ComputedCameraValues,RenderTargetInfo};

    // CPU scheduling/layout fixture only. This writes authored target metadata;
    // it does not run CameraUpdate or establish GPU/window-provider readiness.
    fn publish_targets(windows:Query<&Window,With<PrimaryWindow>>,mut cameras:Query<&mut Camera>) {
        let window=windows.single().unwrap();
        for mut camera in &mut cameras {
            camera.computed=ComputedCameraValues {target_info:Some(RenderTargetInfo {
                physical_size:UVec2::new(window.physical_width(),window.physical_height()),
                scale_factor:window.scale_factor()}),..default()};
        }
    }
    fn scene(width:f32,density:f32,ui_scale:f32,font:bool)->(App,Entity,Entity,classic::tests::Files) {
        scene_with_art(Vec2::new(width,360.), density, ui_scale, font, true)
    }
    fn scene_with_art(size: Vec2, density: f32, ui_scale: f32, font: bool, override_art: bool)
        -> (App, Entity, Entity, classic::tests::Files)
    {
        let (mut app,window)=layout_scene(true,size,density);
        let (files, data) = classic::tests::files_with_art(font, override_art);
        app.insert_resource(data).insert_resource(Assets::<Image>::default()).insert_resource(UiScale(ui_scale));
        app.configure_sets(Update,(crate::PlaySet::Input,crate::PlaySet::Physics,crate::PlaySet::Activation,
            crate::PlaySet::Scripts,crate::PlaySet::Ui,crate::PlaySet::Sync).chain());
        let mut script=crate::script::ScriptState::over_no_plugins();
        script.rt.as_mut().unwrap().game.dialogue=Some(fnv_game::dialogue::Conversation {
            actor:fnv_sim::FormId(42),through:None,line:None,choices:vec![],topic_list:false,
            lines:1,then:None,ending:false,
        });
        app.insert_resource(script).insert_resource(Options {side:Side::Left,device_side:crate::dialogue_split::DeviceSide::Right})
            .insert_resource(SafeInsets {left:12.0,top:6.0,right:10.0,bottom:8.0})
            .add_systems(Update,publish_targets.before(crate::dialogue_split::Resolve));
        app.world_mut().resource_mut::<appearance::Appearance>().theme=appearance::Theme::ClassicGlass;
        app.world_mut().resource_mut::<TouchDialogue>().confirm=false;
        // Preserve the fixture's pre-dialogue default camera through Classic.
        let camera = {
            let mut query = bevy::ecs::system::SystemState::<bevy::ui::DefaultUiCamera>::new(app.world_mut());
            query.get(app.world()).unwrap().get().unwrap()
        };
        app.world_mut().entity_mut(camera).remove::<Camera2d>().insert((
            crate::PlayerCamera, Camera3d::default(), Camera {order:1,..default()},
            Projection::Perspective(PerspectiveProjection::default()),
            Transform::from_xyz(0.0,112.0,0.0),
        ));
        settle(&mut app);
        (app,window,camera,files)
    }
    pub(crate) fn with_barter_surface(test: impl FnOnce(App, Entity)) {
        let (app, window, _, _files) = scene_with_art(Vec2::new(800.,360.), 1., 1., true, false);
        assert_eq!(lease(&app).prepared.layout.template,classic::Template::Wide);
        test(app, window);
    }
    fn lease(app:&App)->crate::dialogue_split::ClassicViewport {
        app.world().resource::<crate::dialogue_split::Frame>().classic.expect("real prepared Classic camera lease")
    }
    fn release(app:&mut App,window:Entity,id:u64,point:Vec2) {
        send(app,window,id,TouchPhase::Started,point);app.update();
        send(app,window,id,TouchPhase::Ended,point);app.update();
    }
    #[cfg(not(target_os = "android"))]
    #[test]
    fn classic_painter_uses_the_bundled_casing_unless_the_user_overrides_it() {
        for (override_art, template, expected_pixel) in [
            (false, classic::Template::Wide, [56, 40, 21, 255]),
            (true, classic::Template::Legacy, [37, 51, 73, 255]),
        ] {
            let (mut app, window, _, files) = scene_with_art(Vec2::new(800.,360.), 1., 1., true, override_art);
            assert_eq!(files.0.join(classic::ASSET).exists(), override_art);
            let frame = *app.world().resource::<crate::dialogue_split::Frame>();
            assert!(ready_for(window, Some(app.world().resource::<Surface>()), &frame));
            let prepared = lease(&app).prepared;
            assert_eq!(prepared.layout.template, template);
            let atlas = prepared.casing;
            let mut nodes = app.world_mut().query::<&ImageNode>();
            assert!(nodes.iter(app.world()).any(|node| node.image.id() == atlas && node.rect.is_some()));
            let assets = app.world().resource::<Assets<Image>>();
            let image = assets.get(atlas).unwrap();
            assert_eq!(image.size(), template.size());
            let pixels = image.data.as_deref().unwrap();
            let at = (100 * template.size().x as usize + 100) * 4;
            assert_eq!(&pixels[at..at + 4], &expected_pixel);
        }
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn bundled_classic_paints_speaker_choices_and_caps_inside_their_openings_at_phone_and_tablet_aspects() {
        for size in [Vec2::new(880.,396.),Vec2::new(845.,390.),Vec2::new(800.,450.),
            Vec2::new(1024.,768.),Vec2::new(393.,873.)] {
            let density=2.0;
            let (mut app,_,_,_files)=scene_with_art(size,density,1.5,true,false);
            let (mut script, mut sim) = crate::menu::classic_dialogue_tests::classic_merchant_world();
            sim.world.add_item(fnv_sim::FormId::PLAYER_REF, crate::menu::model::CAPS, 1234).unwrap();
            let conversation = app.world_mut().resource_mut::<crate::script::ScriptState>()
                .rt.as_mut().unwrap().game.dialogue.take();
            script.rt.as_mut().unwrap().game.dialogue = conversation;
            app.insert_resource(script).insert_resource(sim);
            app.world_mut().resource_mut::<classic_state::State>().spoken = Some("Welcome".into());
            settle(&mut app);
            assert_eq!(app.world().resource::<classic_state::State>().caps, Some(1234));
            let prepared=lease(&app).prepared;
            assert_eq!(prepared.layout.template,classic::Template::Wide);
            let to_window=|r:Rect| Rect::from_corners((r.min+prepared.origin)*density,(r.max+prepared.origin)*density);
            let wells=prepared.layout.wells.map(to_window);
            let mut seen=[0;3];
            let mut glyphs=app.world_mut().query::<(&ImageNode,&ComputedNode,&UiGlobalTransform,Option<&CalculatedClip>)>();
            for (image,node,transform,clip) in glyphs.iter(app.world()) {
                if image.image.id()!=prepared.font || node.is_empty() {continue;}
                let half=node.size()*0.5;
                let mut visible=Rect::from_corners(transform.transform_point2(-half),transform.transform_point2(half));
                if let Some(clip)=clip {
                    let Some(rects)=clip.rects() else {continue;};
                    for rect in rects {
                        // The fixture's UI nodes translate only, so the two
                        // transformed corners are the screen-space clip bounds.
                        let to_window=rect.world_to_clip_local.inverse();
                        visible=visible.intersect(Rect::from_corners(to_window.transform_point2(rect.rect.min),
                            to_window.transform_point2(rect.rect.max)));
                    }
                }
                if visible.size().min_element()<=0. {continue;}
                let index=wells.iter().position(|r| {
                    let r=r.inflate(1.);
                    r.contains(visible.min) && r.contains(visible.max)
                }).unwrap_or_else(||panic!("visible glyph {visible:?} outside Classic openings at {size}"));
                seen[index]+=1;
            }
            assert_eq!(seen[0],15,"speaker and retained spoken line at {size}");
            assert!(seen[1]>=16,"at least the first choice at {size}");
            assert_eq!(seen[2],4,"all four caps glyphs at {size}");
            let first=rows(&mut app)[0].1;
            assert!(first.height()>=48.*density-0.01);
            assert!(wells[1].inflate(1.).contains(first.min) && wells[1].inflate(1.).contains(first.max));
        }
    }

    #[test]
    fn real_vfs_font_and_authored_camera_metadata_publish_safe_classic_rows_at_each_density_and_scale() {
        for (width,density,scale) in [(640.,1.,0.75),(640.,2.,1.5),(800.,1.,1.5),(800.,2.,1.)] {
            let (mut app,window,camera,_files)=scene(width,density,scale,true);
            assert!(app.world().resource::<Cache>().loaded().is_some());
            assert!(app.world().resource::<FontCache>().faces.as_ref().unwrap().text.image.is_some());
            let frame=*app.world().resource::<crate::dialogue_split::Frame>();
            assert!(ready_for(window,Some(app.world().resource::<Surface>()),&frame));
            let request=lease(&app);
            let mut glyphs=app.world_mut().query::<(&ImageNode,&ComputedNode)>();
            assert!(glyphs.iter(app.world()).any(|(image,node)|image.image.id()==request.prepared.font && node.size().min_element()>0.0),"actual loaded Font5 glyph node");
            let actual=app.world().get::<Camera>(camera).unwrap().viewport.as_ref().unwrap();
            assert_eq!((actual.physical_position,actual.physical_size),(request.physical_position,request.physical_size));
            assert_eq!(app.world().get::<Transform>(camera).unwrap().rotation,Quat::IDENTITY);
            let overlay=frame.ui_camera.unwrap();assert_ne!(overlay,camera);
            assert!(app.world().get::<Camera>(overlay).unwrap().viewport.is_none());
            for (_,row) in rows(&mut app) {
                assert!(row.height()>=48.*density-0.01,"real minimum target: width={width} density={density} UiScale={scale} {row:?}");
            }
            let npc=request.prepared.layout.speech.center()+request.prepared.origin;
            release(&mut app,window,70,npc);
            assert!(app.world().resource::<TouchFrame>().asked.pick.is_none());
            assert!(!app.world().resource::<TouchFrame>().asked.skip,"retained NPC text is not a Skip action while choosing");
            let at=rows(&mut app)[0].1.center()/density;release(&mut app,window,71,at);
            assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
            let mut passive=app.world_mut().query::<(&ImageNode,&Pickable)>();
            assert!(passive.iter(app.world()).filter(|(image,_)|image.rect.is_some()).all(|(_,pick)|*pick==Pickable::IGNORE));
        }
        let (app,window,_,_files)=scene(640.,1.,1.,false);
        assert!(app.world().resource::<Cache>().loaded().is_some(),"art alone is insufficient");
        assert!(!ready_for(window,Some(app.world().resource::<Surface>()),app.world().resource::<crate::dialogue_split::Frame>()));
        assert!(app.world().resource::<Surface>().0.is_none(),"missing real Font5 never publishes Classic ready");
    }
    #[test]
    fn classic_lease_restores_owned_viewport_on_exit_and_preserves_foreign_camera_ownership() {
        for cause in 0..7 {
            let (mut app,window,camera,_files)=scene(640.,1.,1.,true);let before=lease(&app);
            match cause {
                0=>{app.insert_resource(State::new(crate::PlayState::Menu));},
                1=>{app.insert_resource(crate::pause::Paused(true));},
                2=>{app.world_mut().get_mut::<Window>(window).unwrap().focused=false;},
                3=>{app.world_mut().resource_mut::<TouchDialogue>().enabled=false;},
                4=>{app.world_mut().resource_mut::<appearance::Appearance>().theme=appearance::Theme::Minimal;},
                5=>{app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.dialogue=None;},
                _=>{app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=false;},
            }
            app.update();
            assert!(app.world().get::<Camera>(camera).unwrap().viewport.is_none(),"owned restore cause={cause} previous={before:?}");
            assert!(app.world().resource::<crate::dialogue_split::Frame>().classic.is_none());
            assert_eq!(app.world().get::<Transform>(camera).unwrap().rotation,Quat::IDENTITY);
        }
        for reset in [false,true] {
            let (mut app,_,camera,_files)=scene(640.,1.,1.,true);
            let foreign=bevy::camera::Viewport {physical_position:UVec2::new(9,11),physical_size:UVec2::new(320,200),..default()};
            let rotation=Quat::from_rotation_y(0.4);
            app.world_mut().get_mut::<Camera>(camera).unwrap().viewport=if reset {None}else{Some(foreign.clone())};
            app.world_mut().get_mut::<Transform>(camera).unwrap().rotation=rotation;
            let default=app.world_mut().spawn((Camera2d,bevy::ui::IsDefaultUiCamera,Camera {order:-5,..default()})).id();
            settle(&mut app);
            let actual=app.world().get::<Camera>(camera).unwrap().viewport.as_ref();
            if reset {assert!(actual.is_none());}else {
                assert_eq!(actual.map(|v|(v.physical_position,v.physical_size)),Some((foreign.physical_position,foreign.physical_size)));
            }
            assert!(app.world().resource::<crate::dialogue_split::Frame>().classic.is_none());
            assert!(app.world().get::<bevy::ui::IsDefaultUiCamera>(default).is_some());
            assert_eq!(app.world().get::<Transform>(camera).unwrap().rotation,rotation);
        }
    }
    #[test]
    fn safe_layout_density_and_ui_scale_changes_retire_old_release_then_current_rows_recover() {
        for cause in 0..3 {
            let (mut app,window,_,_files)=scene(800.,1.,1.,true);let original=lease(&app);
            let at=rows(&mut app)[0].1.center();send(&mut app,window,81,TouchPhase::Started,at);app.update();
            match cause {
                0=>app.world_mut().resource_mut::<SafeInsets>().left+=12.,
                1=>app.world_mut().resource_mut::<UiScale>().0=1.5,
                _=>{
                    let mut w=app.world_mut().get_mut::<Window>(window).unwrap();
                    w.resolution.set_scale_factor_override(Some(2.0));w.resolution.set(800.0,360.0);
                },
            }
            // One ordinary frame publishes the new layout/camera metadata. The
            // old contact cannot authorize a response under the new surface.
            app.update();send(&mut app,window,81,TouchPhase::Ended,at);app.update();
            assert!(app.world().resource::<TouchFrame>().asked.pick.is_none());
            settle(&mut app);assert_ne!(lease(&app),original);
            let density=app.world().get::<Window>(window).unwrap().scale_factor();
            let at=rows(&mut app)[0].1.center()/density;release(&mut app,window,82,at);
            assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
        }
    }
    #[test]
    fn caps_glyphs_fit_inside_the_stretched_counter_opening_for_zero_four_digit_and_extreme_values() {
        let (_files,data)=classic::tests::files(true);let mut images=Assets::<Image>::default();
        let faces=crate::terminal::screen::load_faces(Some(&data),&mut images,false);
        assert!(faces.text.image.is_some(),"real VFS Font5 path, not a default-font substitute");
        for (size,bleed) in [
            (Vec2::new(810.,368.),[32.,0.,30.,24.]), (Vec2::new(813.,370.),[0.,0.,32.,20.]),
            (Vec2::new(800.,450.),[0.;4]), (Vec2::new(1024.,748.),[0.,0.,0.,20.]),
            (Vec2::new(393.,821.),[0.,28.,0.,24.]), (Vec2::new(1920.,1080.),[0.;4]),
        ] {
            for value in [0,9999,i32::MAX,i32::MIN] {
                let text=value.to_string();
                let fit=caps_fit(&faces.text,&text).unwrap();
                let layout=Layout::new(classic::Template::Wide,size,bleed,fit).unwrap();
                assert!(fit.height*layout.caps_size<=layout.caps.height()+0.01,"glyph cell fits the opening height");
                assert!(layout.caps.contains(layout.caps.center()) && layout.wells[2].contains(layout.caps.min)
                    && layout.wells[2].contains(layout.caps.max),"text stays in the counter opening");
                let font=native::scaled_face(&faces.text,layout.caps_size/faces.text.font.size);
                let lines=font.font.lines(&text,Some(layout.caps.width()));
                assert_eq!(lines.len(),1,"caps={value} size={size}");
                assert_eq!(lines[0].text(),text,"complete exact value, never a compact/truncated amount");
                let baseline=crate::interface::line_base(&font);
                for placed in &lines[0].glyphs {
                    let g=font.font.glyph(placed.code);
                    let x=fit.left(layout.caps_size)+placed.x+g.before;
                    assert!(x>=0.0 && x+g.width<=layout.caps.width()+0.01,"caps={value} glyph inside the opening");
                    assert!(baseline-g.ascent>=-0.01 && baseline-g.ascent+g.height<=layout.caps.height()+0.01);
                }
                let overlap=layout.caps.intersect(layout.trade);
                assert!(overlap.width()<=0.0 || overlap.height()<=0.0,"cash and live Trade never share a target");
            }
        }
    }

    #[test]
    fn painted_classic_casing_reaches_every_window_edge_under_the_safe_insets() {
        // The fixture's safe insets are 12, 6, 10 and 8 logical units.
        for (width,density) in [(880.,1.),(800.,2.)] {
            let (mut app,window,_,_files)=scene_with_art(Vec2::new(width,360.),density,1.,true,false);
            let frame=*app.world().resource::<crate::dialogue_split::Frame>();
            assert!(ready_for(window,Some(app.world().resource::<Surface>()),&frame));
            let atlas=lease(&app).prepared.casing;
            let mut pieces=app.world_mut().query::<(&ImageNode,&ComputedNode,&UiGlobalTransform)>();
            let painted:Vec<Rect>=pieces.iter(app.world()).filter(|(image,..)|image.image.id()==atlas)
                .map(|(_,node,t)|Rect::from_center_size(t.transform_point2(Vec2::ZERO),node.size()).inflate(0.5)).collect();
            let size=Vec2::new(width,360.)*density;
            let mut y=0.5;
            while y<size.y {
                let mut x=0.5;
                while x<size.x {
                    assert!(painted.iter().any(|r|r.contains(Vec2::new(x,y))),"no casing at {x},{y} of {size}");
                    x+=3.0;
                }
                y+=3.0;
            }
            // The first response row (the rest scroll inside its clip) stays in the safe canvas.
            let safe=Rect::new(12.,6.,width-10.,352.);
            let row=rows(&mut app)[0].1;
            assert!(safe.contains(row.min/density) && safe.contains(row.max/density));
        }
    }

    #[derive(Resource)]
    struct CameraHandoff {camera:Entity,other_window:Entity,mode:u8,fired:bool,proposal_uncommitted:bool}
    fn handoff_between_resolve_and_admit(mut handoff:ResMut<CameraHandoff>,
        frame:Res<crate::dialogue_split::Frame>,surface:Res<Surface>,
        mut cameras:Query<(&mut Camera,&mut bevy::camera::RenderTarget)>) {
        if handoff.fired {return;}
        handoff.fired=true;
        handoff.proposal_uncommitted=frame.classic.is_none()
            && surface.0.as_ref().is_some_and(|p|!ready_for(p.window,Some(&surface),&frame));
        let (mut camera,mut target)=cameras.get_mut(handoff.camera).unwrap();
        match handoff.mode {
            0=>{},
            1=>camera.viewport=Some(bevy::camera::Viewport {physical_position:UVec2::new(17,19),
                physical_size:UVec2::new(320,170),..default()}),
            2=>*target=bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(handoff.other_window)),
            _=>camera.is_active=false,
        }
    }
    #[test]
    fn actual_camera_handoff_is_admitted_or_refused_before_either_painter_selects_its_frame() {
        for mode in 0..4 {
            let (mut app,window,camera,_files)=scene(800.,1.,1.,true);
            app.init_resource::<crate::pad::PadGlyphs>();
            let original=lease(&app);
            let overlay=app.world().resource::<crate::dialogue_split::Frame>().ui_camera.unwrap();
            let at=rows(&mut app)[0].1.center();
            let other_window=app.world_mut().spawn(Window::default()).id();
            app.insert_resource(CameraHandoff {camera,other_window,mode,fired:false,proposal_uncommitted:false})
                .add_systems(Update,handoff_between_resolve_and_admit
                    .after(crate::dialogue_split::Resolve).before(crate::dialogue_split::Admit));
            send(&mut app,window,91,TouchPhase::Started,at);app.update();
            assert!(app.world().resource::<CameraHandoff>().proposal_uncommitted,
                "Resolve cannot publish proposed Classic as painter-ready: mode={mode}");
            let frame=*app.world().resource::<crate::dialogue_split::Frame>();
            let ready=ready_for(window,Some(app.world().resource::<Surface>()),&frame);
            let classic_roots=app.world_mut().query_filtered::<Entity,With<ClassicRoot>>().iter(app.world()).count();
            let native_roots:Vec<_>=app.world_mut().query_filtered::<(Entity,&UiTargetCamera),
                With<crate::dialogue_menu::TouchNativeRoot>>().iter(app.world()).map(|(e,c)|(e,c.0)).collect();
            if mode==0 {
                assert!(ready && frame.classic==Some(original));
                assert_eq!(classic_roots,1);assert!(native_roots.is_empty());
                let actual=app.world().get::<Camera>(camera).unwrap().viewport.as_ref().unwrap();
                assert_eq!((actual.physical_position,actual.physical_size),(original.physical_position,original.physical_size));
                continue;
            }
            assert!(!ready && frame.classic.is_none());assert_eq!(classic_roots,0);
            assert_eq!(native_roots.len(),1,"real Native fallback must be selected in the same frame, mode={mode}");
            let fallback=native_roots[0].1;assert_ne!(fallback,camera);
            assert_eq!(Some(fallback),frame.ui_camera);assert_eq!(fallback,overlay,"one shared full-window UI facility");
            let fallback_camera=app.world().get::<Camera>(fallback).unwrap();
            assert!(fallback_camera.is_active && fallback_camera.viewport.is_none());
            assert_eq!(app.world().get::<bevy::camera::RenderTarget>(fallback).unwrap().normalize(Some(window)),
                bevy::camera::RenderTarget::default().normalize(Some(window)));
            match mode {
                1=>{
                    let v=app.world().get::<Camera>(camera).unwrap().viewport.as_ref().unwrap();
                    assert_eq!((v.physical_position,v.physical_size),(UVec2::new(17,19),UVec2::new(320,170)));
                },
                2=>assert_eq!(app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().normalize(Some(window)),
                    bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(other_window)).normalize(Some(window))),
                _=>assert!(!app.world().get::<Camera>(camera).unwrap().is_active),
            }
            // The contact began on the old Classic presentation. Fallback cannot
            // reinterpret its terminal release as a new Normal response.
            send(&mut app,window,91,TouchPhase::Ended,at);app.update();
            assert!(app.world().resource::<TouchFrame>().asked.pick.is_none());
            app.world_mut().get_mut::<Camera>(camera).unwrap().viewport=None;
            app.world_mut().get_mut::<Camera>(camera).unwrap().is_active=true;
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap()=bevy::camera::RenderTarget::default();
            app.update();
            assert!(app.world().resource::<crate::dialogue_split::Frame>().classic.is_none(),
                "a foreign takeover is not immediately reacquired while the same Classic view stays open");
            app.world_mut().resource_mut::<appearance::Appearance>().theme=appearance::Theme::Minimal;app.update();
            app.world_mut().resource_mut::<appearance::Appearance>().theme=appearance::Theme::ClassicGlass;settle(&mut app);
            assert!(ready_for(window,Some(app.world().resource::<Surface>()),app.world().resource::<crate::dialogue_split::Frame>()));
            let fresh=rows(&mut app)[0].1.center();release(&mut app,window,92,fresh);
            assert_eq!(app.world().resource::<TouchFrame>().asked.pick,Some(0));
        }
    }

    #[test]
    fn foreign_camera_change_before_scripts_withdraws_old_classic_before_terminal_touch_input() {
        for target_change in [false,true] {
            let (mut app,window,camera,_files)=scene(800.,1.,1.,true);
            app.init_resource::<crate::pad::PadGlyphs>();
            let at=rows(&mut app)[0].1.center();
            send(&mut app,window,101,TouchPhase::Started,at);app.update();
            let other_window=app.world_mut().spawn(Window::default()).id();
            if target_change {
                *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap()=
                    bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(other_window));
            } else {
                app.world_mut().get_mut::<Camera>(camera).unwrap().viewport=Some(bevy::camera::Viewport {
                    physical_position:UVec2::new(21,23),physical_size:UVec2::new(300,150),..default()});
            }
            send(&mut app,window,101,TouchPhase::Ended,at);app.update();
            let asked=&app.world().resource::<TouchFrame>().asked;
            assert!(asked.pick.is_none() && !asked.skip,"the old applied frame cannot authorize release before Resolve: target={target_change}");
            assert!(app.world().resource::<crate::dialogue_split::Frame>().classic.is_none());
            let native=app.world_mut().query_filtered::<&UiTargetCamera,With<crate::dialogue_menu::TouchNativeRoot>>()
                .single(app.world()).unwrap().0;
            assert_eq!(Some(native),app.world().resource::<crate::dialogue_split::Frame>().ui_camera);
            if target_change {
                assert_eq!(app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().normalize(Some(window)),
                    bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(other_window)).normalize(Some(window)));
            } else {
                let viewport=app.world().get::<Camera>(camera).unwrap().viewport.as_ref().unwrap();
                assert_eq!((viewport.physical_position,viewport.physical_size),(UVec2::new(21,23),UVec2::new(300,150)));
            }
        }
    }

}
