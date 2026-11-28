//! Live dialogue content on the existing terminal and Pip-Boy screen renderers.
use super::*;
use crate::dialogue_device::{DeviceKind, DeviceView, Request, Stamp};
use crate::interface::Face;
use nvf_assets::GameData;

#[derive(Component)]
struct Content;
#[derive(Component)]
struct Display;

enum Art {
    Terminal(crate::terminal::screen::Faces),
    PipBoy(Face, Option<crate::pipboy::device_screen::DialogueScreenArt>),
}
impl Art {
    fn face(&self, selected: bool) -> &Face {
        match self {
            Self::Terminal(faces) => if selected {&faces.lit}else{&faces.text},
            Self::PipBoy(face,_) => face,
        }
    }
    fn ink(&self, selected: bool, darkened: bool) -> Color {
        match self {
            Self::Terminal(_) => if selected {crate::terminal::screen::lit_text()}
                else if darkened {crate::terminal::screen::read_green()}
                else {crate::terminal::screen::bright()},
            Self::PipBoy(_,_) => crate::interface::pipboy_amber(if darkened {170.0} else {255.0}),
        }
    }
}
#[derive(Resource,Default)]
pub(crate) struct ArtCache {terminal: Option<Art>, pipboy: Option<Art>}
impl ArtCache {
    /// The device font has been loaded, so [`usable`] measures real text.
    pub(crate) fn ready(&self, kind: DeviceKind) -> bool {
        match kind {DeviceKind::Terminal=>self.terminal.is_some(),DeviceKind::PipBoy=>self.pipboy.is_some()}
    }
}

#[derive(SystemSet,Clone,Copy,Debug,PartialEq,Eq,Hash)]
pub(super) struct Draw;

pub(super) fn install(app: &mut App) {
    app.init_resource::<ArtCache>().add_systems(Update, draw.in_set(crate::PlaySet::Ui).in_set(Draw)
        .after(crate::dialogue_menu::DrawMenu).before(super::native::Draw).before(super::draw));
}

fn line_height(face:&Face)->f32 {
    let descent=face.font.glyphs.iter().chain(face.font.extra.values())
        .map(|glyph|(glyph.height-glyph.ascent).max(0.0)).fold(0.0,f32::max);
    (face.font.size*1.2).max(crate::interface::line_base(face)+descent)
}

fn label_height(face:&Face,text:&str,size:f32,width:f32)->Option<f32> {
    if !face.font.size.is_finite() || face.font.size<=0.0 || !size.is_finite()
        || size<=0.0 || !width.is_finite() || width<=0.0 {return None;}
    let factor=size/face.font.size;
    let lines=face.font.lines(text,Some(width/factor)).len().max(1);
    let height=line_height(face)*factor*lines as f32;
    height.is_finite().then_some(height)
}

/// Use the same real font metrics and reserved areas as the painter. A model
/// cannot qualify merely because its glass has a nominal minimum height.
pub(crate) fn usable(request:&Request,view:&DeviceView,menu:&DialogueMenu,
    settings:&TouchDialogue,cache:Option<&ArtCache>)->bool {
    let Some(content)=menu.view.as_ref() else{return false;};
    let Some(cache)=cache else{return false;};
    let art=match request.stamp.kind {DeviceKind::Terminal=>cache.terminal.as_ref(),DeviceKind::PipBoy=>cache.pipboy.as_ref()};
    let Some(art)=art else{return false;};
    let theme = match request.stamp.kind {
        DeviceKind::Terminal => appearance::Theme::RetroConsole,
        DeviceKind::PipBoy => appearance::Theme::WristTerminal,
    };
    let factor = TouchDialogue::theme_text_factor(theme);
    let face=art.face(false);
    let Some(unit)=view.canvas_length(request,1.0) else{return false;};
    let width=request.canvas.width()-16.0*unit;
    let inner=width-12.0*unit;
    if !inner.is_finite() || width<48.0*unit || inner<=0.0 {return false;}
    let Some(header)=label_height(face,&content.speaker,18.0*factor*unit,width) else{return false;};
    let confirm=settings.confirm && !content.choices.is_empty();
    let footer=if confirm {
        let Some(first)=label_height(face,"Confirm selection",18.0*factor*unit,inner) else{return false;};
        let Some(second)=label_height(face,"Select a choice first",18.0*factor*unit,inner) else{return false;};
        (48.0*unit).max(12.0*unit+first.max(second))
    } else {0.0};
    let remaining=request.canvas.height()-16.0*unit-header-footer
        -(if confirm {8.0}else{4.0})*unit;
    remaining.is_finite() && remaining>=if content.choices.is_empty() {56.0*unit}else{48.0*unit}
}

/// Wrapping and glyph placement use the real device font. Node dimensions are
/// logical UI units; original_units keeps glyph coordinates in the texture.
fn label(parent: &mut ChildSpawnerCommands, text: &str, source: &Face,
    size: f32, width: f32, ui_scale: f32, color: Color) {
    label_on(parent,text,source,size,color,LabelViewport {width, ui_scale, height:960.0});
}

pub(super) struct LabelViewport {
    pub(super) width: f32,
    pub(super) ui_scale: f32,
    pub(super) height: f32,
}

pub(super) fn label_on(parent: &mut ChildSpawnerCommands, text: &str, source: &Face,
    size: f32, color: Color, LabelViewport {width, ui_scale, height}: LabelViewport) {
    let units = 960.0 / height;
    let face = native::scaled_face(source,size*units/source.font.size.max(1.0));
    let lines = face.font.lines(text,Some(width.max(size)*units));
    let line_height = line_height(&face);
    let baseline = crate::interface::line_base(&face);
    parent.spawn((Pickable::IGNORE,Node {width:percent(100.0),
        height:px(line_height * lines.len().max(1) as f32 / units / ui_scale),
        flex_shrink:0.0,..default()})).with_children(|p| {
        let _units = crate::interface::original_units();
        for (index,line) in lines.iter().enumerate() {
            crate::interface::set_line(p,&face,line,0.0,baseline+line_height*index as f32,color);
        }
    });
}

#[derive(Clone, PartialEq)]
struct Key {
    stamp: Stamp,
    scale: f32,
    ui_scale: f32,
    view: crate::dialogue_menu::MenuView,
    list: Option<ListId>,
    selected: usize,
    settings: TouchDialogue,
    armed: bool,
    overlay: Option<Entity>,
    colour: Color,
    publication: crate::dialogue_device::Publication,
}

pub(super) type DrawAssets<'w> = (
    Option<Res<'w, GameData>>,
    Option<ResMut<'w, Assets<Image>>>,
    Option<Res<'w, bevy::image::CompressedImageFormatSupport>>,
);
type OwnedRoots<'w, 's> = Query<'w, 's, Entity, Or<(With<Content>, With<Display>)>>;

#[allow(clippy::too_many_arguments)]
fn draw(mut commands: Commands, menu: Res<DialogueMenu>, settings: Res<TouchDialogue>,
    appearance: Res<appearance::Appearance>, gestures: Res<Gestures>,
    request: Option<Res<Request>>, view: Option<Res<DeviceView>>,
    roots: OwnedRoots<'_, '_>,
    scale: Option<Res<UiScale>>, split: Res<crate::dialogue_split::Frame>,
    (data,images,formats): DrawAssets<'_>,
    (state,paused,script,creator): Availability<'_>,
    mut cache: ResMut<ArtCache>, mut shown: Local<Option<Key>>, mut reveal: ResMut<RevealRequest>,
) {
    let kind = match appearance.theme {appearance::Theme::RetroConsole=>Some(DeviceKind::Terminal),
        appearance::Theme::WristTerminal=>Some(DeviceKind::PipBoy),_=>None};
    let surface = request.as_deref().zip(view.as_deref()).filter(|(request,view)|
        Some(request.stamp.kind)==kind && view.matches(request));
    let ui_scale = scale.as_ref().map_or(1.0,|s|s.0);
    let key = surface.and_then(|(request,view)| menu.view.as_ref().filter(|_|
        settings.enabled && ui_scale.is_finite() && ui_scale>0.0
            && available(state.as_deref(),paused.as_deref(),script.as_deref(),creator.as_deref()))
        .map(|content| Key {stamp:request.stamp,scale:view.scale,ui_scale,view:content.clone(),
            list:menu.list.clone(),selected:menu.selected,settings:*settings,
            armed:gestures.armed==Some(menu.selected),overlay:split.ui_camera,
            colour:match request.stamp.kind {DeviceKind::Terminal=>crate::terminal::screen::bright(),
                DeviceKind::PipBoy=>crate::interface::pipboy_amber(255.0)},publication:view.publication()}));
    if *shown==key {return;}
    for root in &roots {commands.entity(root).despawn();}
    let (Some(key),Some((request,view)),Some(mut images)) = (key,surface,images)
        else {*shown=None;return;};
    let slot = match request.stamp.kind {DeviceKind::Terminal=>&mut cache.terminal,DeviceKind::PipBoy=>&mut cache.pipboy};
    slot.get_or_insert_with(|| {
        let bc = crate::interface::bc_supported(formats.as_deref());
        match request.stamp.kind {
            DeviceKind::Terminal=>Art::Terminal(crate::terminal::screen::load_faces(data.as_deref(),&mut images,bc)),
            DeviceKind::PipBoy=>Art::PipBoy(crate::interface::face(crate::pipboy::frame::FONTS[1],
                crate::interface::plain_font(31.0,13.0),data.as_deref(),&mut images,bc),
                data.as_deref().and_then(|data| crate::pipboy::device_screen::DialogueScreenArt::load(data,&mut images,bc))),
        }
    });
    if !usable(request,view,&menu,&settings,Some(&cache)) {
        *shown=None;
        return;
    }
    let art=match request.stamp.kind {DeviceKind::Terminal=>cache.terminal.as_ref(),DeviceKind::PipBoy=>cache.pipboy.as_ref()}.unwrap();
    spawn(&mut commands,&key,request,view,art);
    reveal.0=true;
    *shown=Some(key);
}

fn spawn(commands: &mut Commands, key: &Key, request: &Request, view: &DeviceView, art: &Art) {
    let units=key.ui_scale;
    let rect=key.stamp.display;
    let mut display=commands.spawn((Display,PaletteRoot,Pickable::IGNORE,GlobalZIndex(2400),
        Node {position_type:PositionType::Absolute,left:px(rect.min.x/key.stamp.density/units),
            top:px(rect.min.y/key.stamp.density/units),width:px(rect.width()/key.stamp.density/units),
            height:px(rect.height()/key.stamp.density/units),..default()},ImageNode::new(view.image.clone())));
    if let Some(camera)=key.overlay {display.insert(UiTargetCamera(camera));}
    // Power-button exit hit-testing (`view.power_button_at`) stays active directly
    // over the physical 3D power button with no visible circle/glyph overlay.
    let solid_backdrop = match request.stamp.kind {
        DeviceKind::PipBoy => {
            let [r, g, b] = crate::interface::pipboy_rgb();
            Color::srgb(
                0.014 + f32::from(r) / 255.0 * 0.032,
                0.014 + f32::from(g) / 255.0 * 0.032,
                0.010 + f32::from(b) / 255.0 * 0.022,
            )
        }
        DeviceKind::Terminal => Color::srgb(0.012, 0.042, 0.020),
    };
    commands.spawn((Content,PaletteRoot,UiTargetCamera(request.camera),Node {
        position_type:PositionType::Absolute,left:px(request.canvas.min.x/units),
        top:px(request.canvas.min.y/units),width:px(request.canvas.width()/units),
        height:px(request.canvas.height()/units),overflow:Overflow::clip(),..default()},
        BackgroundColor(solid_backdrop)))
        .with_children(|root| {
            {
                let _units=crate::interface::original_units();
                match art {
                    Art::Terminal(faces)=>crate::terminal::screen::spawn_background(root,faces,request.canvas),
                    Art::PipBoy(_,Some(art))=>art.spawn(root,request.canvas),
                    _=>{},
                }
            }
            spawn_rows(root,key,request,view,art);
            if let Art::Terminal(faces)=art {
                let _units=crate::interface::original_units();
                crate::terminal::screen::spawn_scanlines(root,faces,request.canvas);
            }
        });
}

fn spawn_rows(root: &mut ChildSpawnerCommands, key: &Key, request: &Request, view: &DeviceView, art: &Art) {
    let theme = match request.stamp.kind {
        DeviceKind::Terminal => appearance::Theme::RetroConsole,
        DeviceKind::PipBoy => appearance::Theme::WristTerminal,
    };
    let factor = TouchDialogue::theme_text_factor(theme);
    let length=|logical|view.canvas_length(request,logical).unwrap();
    let units=key.ui_scale;
    let padding=length(8.0);
    let row_padding=length(6.0);
    let width=request.canvas.width()-2.0*padding;
    let height=request.canvas.height()-2.0*padding;
    root.spawn(Node {position_type:PositionType::Absolute,left:px(padding/units),top:px(padding/units),
        width:px(width/units),height:px(height/units),flex_direction:FlexDirection::Column,
        row_gap:px(length(4.0)/units),..default()}).with_children(|p| {
        label(p,&key.view.speaker,art.face(false),length(18.0*factor),width,units,art.ink(false,false));
        p.spawn((Viewport,ScrollPosition::default(),Node {width:percent(100.0),flex_grow:1.0,
            min_height:px(0.0),overflow:Overflow::scroll_y(),flex_direction:FlexDirection::Column,..default()}))
            .with_children(|p| {
                p.spawn(Node {width:percent(100.0),flex_shrink:0.0,flex_direction:FlexDirection::Column,
                    row_gap:px(length(4.0)/units),..default()}).with_children(|p| {
                    if let Some(line)=&key.view.line {
                        p.spawn((Button,native_skip(key.list.clone()),Node {width:percent(100.0),
                            min_height:px(length(56.0)/units),padding:UiRect::all(px(row_padding/units)),
                            flex_shrink:0.0,..default()})).with_children(|p|label(p,line,art.face(false),
                                length(key.settings.subtitle_size_for(theme)),width-2.0*row_padding,
                                units,art.ink(false,false)));
                    }
                    for (index,row) in key.view.choices.iter().enumerate() {
                        let selected=index==key.selected;
                        let fill=match art {
                            Art::Terminal(_) if selected=>crate::terminal::screen::bright(),
                            Art::PipBoy(_,_) if selected=>art.ink(false,false).with_alpha(0.12),
                            _=>Color::NONE,
                        };
                        let line=if row.short {appearance::check_line()}else{art.ink(false,false)};
                        p.spawn((Button,native_choice(index,key.list.clone(),row.short),Node {
                            width:percent(100.0),min_height:px(length(48.0)/units),
                            padding:UiRect::all(px(row_padding/units)),flex_shrink:0.0,..default()},
                            BackgroundColor(fill),Outline::new(px(1.0/units),px(-1.0/units),
                                if selected && matches!(art,Art::PipBoy(_, _)) {line}else{Color::NONE})))
                            .with_children(|p|label(p,&row.label,art.face(selected),
                                length(key.settings.choice_size_for(theme)),width-2.0*row_padding,
                                units,art.ink(selected,row.darkened)));
                    }
                });
            });
        if key.settings.confirm && !key.view.choices.is_empty() {
            p.spawn((Button,native_confirm(key.list.clone()),Node {width:percent(100.0),
                min_height:px(length(48.0)/units),padding:UiRect::all(px(row_padding/units)),
                flex_shrink:0.0,..default()})).with_children(|p|label(p,
                    if key.armed {"Confirm selection"}else{"Select a choice first"},
                    art.face(false),length(18.0*factor),width-2.0*row_padding,units,art.ink(false,false)));
        }
    });
}

#[cfg(test)]
mod tests;
