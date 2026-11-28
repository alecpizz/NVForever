//! The normal touch view uses the game's existing dialogue painter and asset kit.
use super::*;
use crate::dialogue_menu::{ChoicePlace, Kit, Layout, ListPlace, Placed, ScrollBar, TouchNativeRoot};
use crate::interface::Face;

#[derive(SystemSet,Clone,Copy,Debug,PartialEq,Eq,Hash)]
pub(super) struct Draw;

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, draw.in_set(crate::PlaySet::Ui).in_set(Draw)
        .after(crate::dialogue_menu::DrawMenu).before(super::draw));
}

pub(super) fn uses_normal(theme: appearance::Theme, device_ready: bool, classic_ready: bool) -> bool {
    match theme {
        appearance::Theme::Minimal => true,
        appearance::Theme::ClassicGlass => !classic_ready,
        appearance::Theme::RetroConsole | appearance::Theme::WristTerminal => !device_ready,
        appearance::Theme::SplitPanel => false,
    }
}

pub(super) fn scaled_face(source: &Face, factor: f32) -> Face {
    let mut face = source.clone();
    face.font.size *= factor;
    for glyph in face.font.glyphs.iter_mut().chain(face.font.extra.values_mut()) {
        glyph.width *= factor;
        glyph.height *= factor;
        glyph.before *= factor;
        glyph.after *= factor;
        glyph.ascent *= factor;
    }
    if let Some(vector) = &mut face.vector {
        vector.em *= factor;
        vector.base *= factor;
    }
    face
}

fn layout(view: &crate::dialogue_menu::MenuView, kit: &Kit, width: f32, row_min: f32, confirm: bool) -> Layout {
    let band_width = (width - 80.0).clamp(240.0, 1080.0);
    let band_x = (width - band_width) * 0.5;
    let name = (!view.speaker.trim().is_empty()).then(|| {
        let line = kit.name.font.lines(view.speaker.trim(), Some(width - 100.0)).remove(0);
        Placed {x: width - 50.0 - line.width, base: 200.0, line}
    });
    let mut out = Layout {name, band: None, text: Vec::new(), list: None,
        hint: None, hint_back: None, first: 0};
    let baseline = crate::interface::line_base(&kit.body);
    let line_height = kit.body.font.size * 1.2;
    if view.choices.is_empty() {
        let Some(line) = view.line.as_deref() else {return out;};
        let lines = kit.body.font.lines(line, Some(band_width - 100.0));
        let height = line_height * lines.len().max(1) as f32;
        let top = (880.0 - height).max(240.0);
        out.band = Some([band_x,top-25.0,band_width,height+50.0]);
        out.text = lines.into_iter().enumerate().map(|(index,line)| Placed {
            x:band_x+50.0,base:top+baseline+index as f32*line_height,line}).collect();
        return out;
    }
    let left = band_x + 52.0;
    let choice_width = band_width - 77.0;
    let wrap = choice_width - 40.0;
    let rows: Vec<_> = view.choices.iter().map(|row| {
        let lines = kit.body.font.lines(row.label.trim(), Some(wrap));
        let height = (line_height * lines.len().max(1) as f32 + 28.0).max(row_min);
        (lines,height)
    }).collect();
    let total: f32 = rows.iter().map(|(_,height)|*height).sum();
    let footer = if confirm {row_min+20.0}else{0.0};
    let height = total.min(610.0-footer).max(row_min.min(610.0-footer));
    let top = 885.0 - height - footer;
    let mut y = top;
    let choices = rows.into_iter().enumerate().map(|(index,(lines,height))| {
        let placed = lines.into_iter().enumerate().map(|(line_index,line)| Placed {
            x:left+20.0,base:y+14.0+baseline+line_index as f32*line_height,line}).collect();
        let row = ChoicePlace {index,area:[left,y,choice_width,height],lines:placed};
        y += height;
        row
    }).collect();
    let bottom = top + height;
    out.band = Some([band_x,top-45.0,band_width,height+90.0+footer]);
    out.list = Some(ListPlace {area:[band_x+20.0,top,band_width-40.0,height],
        top_line:top-20.0,bottom_line:bottom+10.0,bracket_x:band_x+35.0,
        bracket_width:band_width-55.0,choices,
        scroll:(total>height).then_some(ScrollBar {
            up:[band_x+20.0,top+16.0,32.0,16.0],
            down:[band_x+20.0,bottom-32.0,32.0,16.0],
            marker:[band_x+32.0,top+55.0,6.0,40.0],
        })});
    out
}

#[derive(Clone, PartialEq)]
struct Key {
    window: Entity,
    camera: Entity,
    view: crate::dialogue_menu::MenuView,
    list: Option<ListId>,
    selected: usize,
    size: Vec2,
    settings: TouchDialogue,
    armed: bool,
    data: bool,
    ui_scale: f32,
    colour: [u8;3],
    labels: crate::pad::Labels,
}

type ModelSurfaces<'w> = (
    Option<Res<'w, crate::dialogue_device::Request>>,
    Option<Res<'w, crate::dialogue_device::DeviceView>>,
    Option<Res<'w, classic_presenter::Surface>>,
    Res<'w, crate::dialogue_split::Frame>,
    Option<Res<'w, device_canvas::ArtCache>>,
);

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands, menu: Res<DialogueMenu>, touch: Res<TouchDialogue>,
    appearance: Res<appearance::Appearance>, gestures: Res<Gestures>,
    (data,images,formats): device_canvas::DrawAssets<'_>,
    windows: Query<(Entity,&Window),With<PrimaryWindow>>, roots: Query<Entity,With<TouchNativeRoot>>,
    (state,paused,script,creator): Availability<'_>,
    scale: Option<Res<UiScale>>, glyphs: Option<Res<crate::pad::PadGlyphs>>,
    mut kit: Local<Option<(bool,Kit)>>, mut shown: Local<Option<Key>>,
    mut reveal: ResMut<RevealRequest>,
    (request,device,classic,split,art): ModelSurfaces<'_>,
) {
    let expected=match appearance.theme {
        appearance::Theme::RetroConsole=>Some(crate::dialogue_device::DeviceKind::Terminal),
        appearance::Theme::WristTerminal=>Some(crate::dialogue_device::DeviceKind::PipBoy),_=>None,
    };
    let ui_scale=scale.as_ref().map_or(1.0,|s|s.0);
    let key = windows.single().ok().and_then(|(entity,window)| {
        let camera = split.ui_camera?;
        if images.is_none() || glyphs.is_none() {return None;}
        let device_ready=request.as_deref().zip(device.as_deref()).is_some_and(|(request,view)|
            Some(request.stamp.kind)==expected && request.stamp.window==entity && view.matches(request)
                && device_canvas::usable(request,view,&menu,&touch,art.as_deref()));
        menu.view.as_ref().filter(|_|
        touch.enabled && ui_scale.is_finite() && ui_scale>0.0
            && window.width().is_finite() && window.height().is_finite()
            && window.width()>0.0 && window.height()>0.0
            && uses_normal(appearance.theme,device_ready,
                classic_presenter::ready_for(entity,classic.as_deref(),&split))
            && available(state.as_deref(),paused.as_deref(),script.as_deref(),creator.as_deref()))
        .map(|view| Key {window:entity,camera,view:view.clone(),list:menu.list.clone(),selected:menu.selected,
            size:Vec2::new(window.width(),window.height()),settings:*touch,
            armed:gestures.armed==Some(menu.selected),data:data.is_some(),
            ui_scale,colour:crate::interface::hud_rgb(),labels:crate::pad::labels()})});
    if *shown == key {return;}
    for entity in &roots {commands.entity(entity).despawn();}
    let Some(key) = key else {*shown=None;return;};
    let (Some(mut images),Some(glyphs)) = (images,glyphs) else {*shown=None;return;};
    if kit.as_ref().is_none_or(|(loaded,_)|*loaded!=key.data) {
        *kit = Some((key.data,crate::dialogue_menu::load_kit(data.as_deref(),&mut images,
            crate::interface::bc_supported(formats.as_deref()))));
    }
    let (_,source) = kit.as_ref().unwrap();
    let height = key.size.y.max(1.0);
    let units = 960.0 / height;
    let size = if key.view.choices.is_empty() {touch.subtitle_size()} else {touch.choice_size()};
    let body = scaled_face(&source.body,size*units/source.body.font.size.max(1.0));
    let name = scaled_face(&source.name,18.0*units/source.name.font.size.max(1.0));
    // Reuse the native kit's pictures, glyphs, fade brackets and selection painter.
    let mut adapted = source.clone();
    adapted.body = body;
    adapted.name = name;
    let geometry = layout(&key.view,&adapted,960.0*key.size.x/height,48.0*units,touch.confirm);
    let caption = touch.confirm.then_some((if key.armed {"Confirm selection"}else{"Select a choice first"},48.0*units));
    {
        let _units = crate::interface::original_units();
        crate::dialogue_menu::spawn_touch_menu(&mut commands,&key.view,&geometry,&adapted,
            menu.selected,menu.list.as_ref(),&glyphs,caption,key.camera);
    }
    reveal.0 = true;
    *shown = Some(key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_touch::compact_tests::{layout_scene, settle};
    use bevy::ui::{ComputedUiTargetCamera, IsDefaultUiCamera};

    fn published_camera(app: &mut App) -> (Entity, Entity) {
        let world = app.world_mut();
        let (root, target) = world.query_filtered::<(Entity, &UiTargetCamera), With<TouchNativeRoot>>()
            .single(world).unwrap();
        let camera = target.0;
        assert_eq!(world.resource::<crate::dialogue_split::Frame>().ui_camera, Some(camera));
        assert_eq!(world.get::<ComputedUiTargetCamera>(root).unwrap().get(), Some(camera));
        let published = world.get::<Camera>(camera).unwrap();
        assert!(published.is_active && published.viewport.is_none());
        (root, camera)
    }

    #[test]
    fn unavailable_wrist_terminal_normal_fallback_scrolls_and_selects_revealed_choice() {
        use crate::dialogue_touch::compact_tests::{rows, send};
        let (mut app, window) = layout_scene(true, Vec2::new(792.0, 363.0), 2.0);
        app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
        app.world_mut().resource_mut::<TouchDialogue>().confirm = false;
        let theme = app.world().resource::<crate::settings::Settings>()
            .index(appearance::THEME_KEY).unwrap();
        assert!(crate::settings::pick(app.world_mut(), theme, "Wrist Terminal"));
        settle(&mut app);
        published_camera(&mut app);
        assert!(!app.world().contains_resource::<crate::dialogue_device::DeviceView>());
        let before = rows(&mut app);
        let start = before[1].1.center() / 2.0;
        let end = start - Vec2::Y * 160.0;
        send(&mut app, window, 31, TouchPhase::Started, start);
        app.update();
        send(&mut app, window, 31, TouchPhase::Moved, end);
        app.update();
        send(&mut app, window, 31, TouchPhase::Ended, end);
        app.update();
        assert!(app.world().resource::<TouchFrame>().asked.pick.is_none());
        let after = rows(&mut app);
        assert!(after[0].1.center().y < before[0].1.center().y - 100.0,
            "the actual painted row transforms move rather than merely updating ScrollPosition");
        let point = after[5].1.center() / 2.0;
        send(&mut app, window, 32, TouchPhase::Started, point);
        send(&mut app, window, 32, TouchPhase::Ended, point);
        app.update();
        assert_eq!(app.world().resource::<TouchFrame>().asked.pick, Some(5));
        assert_eq!(app.world().resource::<appearance::Appearance>().theme, appearance::Theme::WristTerminal);
    }

    #[test]
    fn normal_palette_change_is_painted_once_at_the_new_hud_colour() {
        let _palette = crate::interface::colours_at([80,100,120], [30,60,90]);
        let (mut app, _) = layout_scene(true, Vec2::new(792.,363.), 1.);
        app.edit_schedule(Update, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>()
            .add_plugins(crate::ui_colour::UiColourPlugin);
        app.world_mut().resource_mut::<appearance::Appearance>().theme = appearance::Theme::Minimal;
        settle(&mut app);
        crate::interface::set_hud_rgb([40,50,60]);
        settle(&mut app);
        let (root, _) = published_camera(&mut app);
        assert!(app.world().get::<PaletteRoot>(root).is_some());
        let expected = crate::interface::amber(255.0);
        let world = app.world_mut();
        let mut ink = world.query::<(Entity,Option<&ImageNode>,Option<&TextColor>)>();
        assert!(ink.iter(world).any(|(entity,image,text)| {
            let mut cursor = entity;
            let mut owned = false;
            for _ in 0..64 {
                if cursor == root {owned=true;break;}
                let Some(parent) = world.get::<ChildOf>(cursor) else {break;};
                cursor = parent.parent();
            }
            owned && (image.is_some_and(|i|i.color==expected)
                || text.is_some_and(|t|t.0==expected))
        }), "native glyphs use the new colour without a second generic retint");
    }

    #[test]
    fn normal_retargets_replaced_overlay_and_leaves_unmarked_hud_on_ordinary_camera() {
        let (mut app, _) = layout_scene(true, Vec2::new(792., 363.), 1.);
        app.world_mut().resource_mut::<TouchDialogue>().enabled = false;
        let viewport = bevy::camera::Viewport { physical_position: UVec2::new(12, 4),
            physical_size: UVec2::new(320, 220), ..default() };
        let ordinary = app.world_mut().spawn((crate::PlayerCamera, Camera3d::default(),
            Camera { order: 3, viewport: Some(viewport.clone()), ..default() })).id();
        let hud = app.world_mut().spawn(Node::default()).id();
        app.init_resource::<Assets<Image>>().init_resource::<crate::pad::PadGlyphs>();
        settle(&mut app);
        assert!(app.world().get::<IsDefaultUiCamera>(ordinary).is_none());
        assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(), Some(ordinary));
        app.world_mut().resource_mut::<appearance::Appearance>().theme = appearance::Theme::Minimal;
        app.world_mut().resource_mut::<TouchDialogue>().enabled = true;
        settle(&mut app);
        let (old_root, old_camera) = published_camera(&mut app);
        // Field Notes must not repaint native rows merely because Node requires
        // BackgroundColor and BorderColor components on every UI element.
        let world = app.world_mut();
        let mut rows = world.query_filtered::<&BackgroundColor, With<Row>>();
        assert!(rows.iter(world).count() > 0);
        assert!(rows.iter(world).all(|background| background.0 == Color::NONE));
        assert_ne!(old_camera, ordinary);
        assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(), Some(ordinary));
        // Unchanged text and selection must move with the camera owner's replacement.
        app.world_mut().despawn(old_camera);
        settle(&mut app);
        let (new_root, new_camera) = published_camera(&mut app);
        assert_ne!(new_camera, old_camera);
        assert_ne!(new_root, old_root);
        assert!(app.world().get_entity(old_root).is_err());
        assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(), Some(ordinary));
        app.world_mut().resource_mut::<TouchDialogue>().enabled = false;
        settle(&mut app);
        assert!(app.world().get_entity(new_root).is_err());
        assert!(app.world().get_entity(new_camera).is_err());
        assert!(app.world().get::<IsDefaultUiCamera>(ordinary).is_none());
        assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(), Some(ordinary));
        let after = app.world().get::<Camera>(ordinary).unwrap().viewport.as_ref().unwrap();
        assert_eq!((after.physical_position, after.physical_size, &after.depth),
            (viewport.physical_position, viewport.physical_size, &viewport.depth));
    }
}
