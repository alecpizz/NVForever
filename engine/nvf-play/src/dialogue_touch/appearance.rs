//! Borderless dialogue treatments over the live world, with independent accents and tint.
use bevy::prelude::*;
#[cfg(any(target_os = "android", test))]
use crate::settings::{add_choice_list, add_setting, ChoiceList, Setting, Slider};

pub(crate) const THEME_KEY: &str = "dialogue-theme-v1";
pub(crate) const BACKGROUND_KEY: &str = "dialogue-background-opacity-v1";
pub(crate) const TINT_KEY: &str = "dialogue-background-tint-v1";
pub(crate) const FRAME_KEY: &str = "dialogue-frame-opacity-v1";
/// Conservative room for two16-unit lines at1.2line height plus padding/borders.
pub(crate) const MIN_PREVIEW_UNITS: f32 = 78.0;
pub(crate) const HINTS_KEY: &str = "dialogue-hints-v1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Theme {
    #[default] Minimal, ClassicGlass, SplitPanel, RetroConsole, WristTerminal,
}
#[cfg(any(target_os = "android", test))]
impl Theme {
    const ALL: [Self; 5] = [Self::Minimal, Self::ClassicGlass, Self::SplitPanel, Self::RetroConsole, Self::WristTerminal];
    fn name(self) -> &'static str {
        match self { Self::Minimal => "Normal", Self::ClassicGlass => "Classic", Self::SplitPanel => "Field Notes", Self::RetroConsole => "Retro Console", Self::WristTerminal => "Wrist Terminal" }
    }
}
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Appearance {
    pub theme: Theme,
    pub background: f32,
    pub frame: f32,
    pub hints: bool,
    pub tint: Tint,
}
impl Default for Appearance {
    fn default() -> Self { Self { theme: Theme::Minimal, background: 0.18, frame: 0.60, hints: false, tint: Tint::Hud } }
}
#[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tint { #[default] Hud, Neutral, Amber }
#[cfg(any(target_os = "android", test))]
fn tint_name(w:&World)->String {match w.resource::<Appearance>().tint {Tint::Hud=>"HUD colour",Tint::Neutral=>"Neutral",Tint::Amber=>"Warm amber"}.into()}
#[cfg(any(target_os = "android", test))]
fn pick_tint(w:&mut World,name:&str)->bool {
    let tint=match name {"HUD colour"=>Tint::Hud,"Neutral"=>Tint::Neutral,"Warm amber"=>Tint::Amber,_=>return false};
    w.resource_mut::<Appearance>().tint=tint;true
}
fn opacity(value: f32) -> f32 { if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 } }
fn backing(value: f32) -> f32 { opacity(value).min(0.80) }
#[cfg(any(target_os = "android", test))]
fn shown(value: f32) -> String { format!("{:.0}%", opacity(value) * 100.0) }
#[cfg(any(target_os = "android", test))]
fn shown_backing(value: f32) -> String { shown(opacity(value)*0.80) }
#[cfg(any(target_os = "android", test))]
fn chosen(world: &World) -> String { world.resource::<Appearance>().theme.name().into() }
#[cfg(any(target_os = "android", test))]
fn pick(world: &mut World, name: &str) -> bool {
    let name=match name {"Split Panel"=>"Field Notes","Minimal"=>"Normal","Classic Glass"=>"Classic",_=>name};
    let Some(theme) = Theme::ALL.into_iter().find(|t| t.name() == name) else { return false; };
    world.resource_mut::<Appearance>().theme = theme;
    true
}
#[cfg(any(target_os = "android", test))]
const THEME_NOTE: &str = "Choose standard dialogue, Field Notes, the Classic casing, or an installed Terminal or Pip-Boy model.";
#[cfg(any(target_os = "android", test))]
fn theme_setting(note: &'static str) -> Setting {
    Setting { key: THEME_KEY, group: crate::pause::GROUP_MENUS, label: "Dialogue theme",
        note, switches: "", get: |_|false, set: |_,_|{}, slider: None }
}
#[cfg(any(target_os = "android", test))]
fn theme_choices() -> ChoiceList {
    ChoiceList { names: |_|Theme::ALL.iter().map(|t|t.name().into()).collect(), chosen, pick, save: chosen, load: pick, ask_keep: false }
}
/// Register the real choices/callbacks with an authored note before the full installer.
/// A pre-existing ordinary row would intentionally prevent list registration.
#[cfg(test)]
pub(crate) fn register_theme_note(app: &mut App, note: &'static str) {
    app.init_resource::<Appearance>();
    add_choice_list(app, theme_setting(note), theme_choices());
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<Appearance>().add_systems(Update, (paint, markers, instrument, preview_visibility).chain().after(crate::PlaySet::Ui).after(crate::ui_colour::Recolour).before(crate::PlaySet::Sync));
    #[cfg(any(target_os = "android", test))]
    {
        add_choice_list(app, theme_setting(THEME_NOTE), theme_choices());
        add_choice_list(app, Setting {key:TINT_KEY,group:crate::pause::GROUP_MENUS,label:"Dialogue background tint",
            note:"Tint the translucent choice backing. Text and selection still use your preferred HUD colour.",switches:"",get:|_|false,set:|_,_|{},slider:None},
            ChoiceList {names:|_|["HUD colour","Neutral","Warm amber"].map(str::to_owned).to_vec(),chosen:tint_name,pick:pick_tint,save:tint_name,load:pick_tint,ask_keep:false});
        for (key,label,frame) in [(BACKGROUND_KEY,"Dialogue background opacity",false),(FRAME_KEY,"Dialogue accent opacity",true)] {
            let slider = if frame {
                Slider { get: |w|w.resource::<Appearance>().frame, set: |w,v|w.resource_mut::<Appearance>().frame=opacity(v), min:0.0, step:0.01, shown:Some(shown) }
            } else {
                Slider { get: |w|w.resource::<Appearance>().background/0.80, set: |w,v|w.resource_mut::<Appearance>().background=backing(opacity(v)*0.80), min:0.0, step:0.01, shown:Some(shown_backing) }
            };
            add_setting(app, Setting { key, group:crate::pause::GROUP_MENUS,label,
                note:"Live appearance only. Backing is capped at 80% so the scene remains visible. Text and selection cues keep their contrast; targets and input stay unchanged.",switches:"",get:|_|false,set:|_,_|{},slider:Some(slider) });
        }
        add_setting(app, Setting {key:HINTS_KEY,group:crate::pause::GROUP_MENUS,label:"Show dialogue hints",
            note:"Show gesture and tutorial captions in enhanced dialogue. NPC speech, choices, requirements and selected-action labels always remain visible.",switches:"",
            get:|w|w.resource::<Appearance>().hints,set:|w,v|w.resource_mut::<Appearance>().hints=v,slider:None});
    }
}

/// All theme decoration is paint-only: never Button, Action or an input owner.
#[derive(Component, Clone, Copy)]
pub(super) enum Paint {
    ResponseDetail {index:usize,part:u8}, ResponseMark(usize), Panel, Speaker, Speech, Hint, ChoiceText { darkened: bool },
    Choice { index: usize }, Focus { index: usize, short: bool }, Confirm, Caption, PreviewSelected, PreviewFocus, Trim(u8),
}
#[derive(Component)]
pub(crate) struct Preview;
#[derive(Component)]
pub(crate) struct PreviewSelection;
// Existing unmet-check feedback is semantic, not a selectable theme colour.
pub(super) fn check_line() -> Color { Color::srgb(0.8,0.2,0.15) }
fn palette(rgb: [u8;3], brightness: f32, alpha: f32) -> Color {
    crate::interface::tint(rgb,brightness).with_alpha(opacity(alpha))
}
impl Appearance {
    pub(super) fn response_colors(self,_short:bool,selected:bool)->(Color,Color) {
        let (_,fill,line)=self.colors(Paint::Choice{index:0},if selected {0}else{1},crate::interface::hud_rgb());
        (line.unwrap_or(Color::NONE),fill.unwrap_or(Color::NONE))
    }
    fn line(self, rgb:[u8;3], factor:f32) -> Color { palette(rgb,255.0,self.frame*factor) }
    fn fill(self, rgb:[u8;3], brightness:f32, factor:f32) -> Color { palette(match self.tint {Tint::Hud=>rgb,Tint::Neutral=>[255,255,255],Tint::Amber=>[255,181,74]},brightness,backing(self.background)*factor) }
    fn colors(self, kind:Paint, selected:usize, rgb:[u8;3]) -> (Option<Color>,Option<Color>,Option<Color>) {
        match kind {
            Paint::ResponseDetail{index,part:_} => {
                let alpha=match self.theme {Theme::RetroConsole=>0.08,Theme::WristTerminal=>if index==selected {0.65}else{0.18},_=>0.0};
                (None,Some(self.line(rgb,alpha)),None)
            }
            Paint::Panel => (None,Some(Color::NONE),Some(Color::NONE)),
            Paint::ResponseMark(_) | Paint::Speaker | Paint::Speech | Paint::Caption => (Some(palette(rgb,255.,1.)),None,None),
            Paint::ChoiceText{darkened} => (Some(palette(rgb,if darkened {160.}else{255.},1.)),None,None),
            Paint::Hint => (Some(palette(rgb,190.,1.)),None,None),
            Paint::Choice{index} => {
                let on=index==selected;
                let (brightness,factor)=match self.theme {
                    Theme::Minimal=>(8.,if on {0.55}else{0.0}),
                    Theme::ClassicGlass=>(48.,if on {1.0}else{0.0}),
                    Theme::SplitPanel=>(12.,if on {0.65}else{0.0}),
                    Theme::RetroConsole=>(24.,if on {0.8}else{0.15}),
                    Theme::WristTerminal=>(36.,if on {1.0}else{0.2}),
                };
                (None,Some(self.fill(rgb,brightness,factor)),Some(Color::NONE))
            }
            Paint::Focus{index,short} => (None,Some(if index==selected && short {check_line()}else{palette(rgb,255.,if index==selected {1.}else{0.})}),None),
            Paint::PreviewFocus => (None,Some(palette(rgb,255.,1.)),None),
            Paint::Confirm | Paint::PreviewSelected => (None,Some(self.fill(rgb,48.,1.0)),Some(Color::NONE)),
            Paint::Trim(n) => {
                let visible=match self.theme {
                    Theme::Minimal | Theme::ClassicGlass => false,
                    Theme::SplitPanel => n==4,
                    Theme::RetroConsole => n==6,
                    Theme::WristTerminal => n>=6,
                };
                (None,Some(self.line(rgb,if visible {1.0}else{0.0})),None)
            }
        }
    }
}
type Painted = (&'static Paint,Option<&'static mut TextColor>,Option<&'static mut BackgroundColor>,Option<&'static mut BorderColor>,Option<&'static mut Node>,Option<&'static mut ImageNode>);
fn paint(
    appearance: Res<Appearance>, menu: Res<super::DialogueMenu>,
    mut nodes: Query<Painted>,
) {
    let rgb=crate::interface::hud_rgb();
    for (kind,text,back,border,node,image) in &mut nodes {
        let (t,b,line)=appearance.colors(*kind,menu.selected,rgb);
        if let (Some(mut value),Some(color))=(text,t) { if value.0!=color {value.0=color;} }
        let has_image=image.is_some();
        if let (Some(mut value),Some(color))=(image,b) {if value.color!=color {value.color=color;}}
        if let (Some(mut value),Some(color))=(back,b) {
            let color=if has_image {Color::NONE}else{color};
            if value.0!=color {value.0=color;}
        }
        if let (Some(mut value),Some(color))=(border,line) { let next=BorderColor::all(color);if *value!=next {*value=next;} }
        if let Some(mut node)=node {
            if let Paint::ResponseDetail{part,..}=kind {
                let next=if appearance.theme==Theme::WristTerminal {(px(2.),Val::Auto,px(5.),px(10.+f32::from(*part)*7.))}
                    else {(px(30.),px(4.),Val::Auto,percent(25.+f32::from(*part)*25.))};
                if (node.left,node.right,node.width,node.top)!=next {node.left=next.0;node.right=next.1;node.width=next.2;node.top=next.3;}
            }
            // Accents change the visual vocabulary without changing the response hit box.
            if matches!(kind,Paint::Focus{..}|Paint::PreviewFocus) {
                let (left,top,bottom,width,height)=match appearance.theme {
                    Theme::Minimal=>(px(8.),Val::Auto,px(2.),percent(35.),px(1.)),
                    Theme::ClassicGlass=>(px(2.),px(8.),px(8.),px(2.),Val::Auto),
                    Theme::SplitPanel=>(px(8.),Val::Auto,px(3.),px(16.),px(1.)),
                    Theme::RetroConsole=>(px(9.),Val::Auto,px(7.),px(7.),px(2.)),
                    Theme::WristTerminal=>(px(2.),px(8.),px(8.),px(3.),Val::Auto),
                };
                if (node.left,node.top,node.bottom,node.width,node.height)!=(left,top,bottom,width,height) {
                    node.left=left;node.top=top;node.bottom=bottom;node.width=width;node.height=height;
                }
            }
            if matches!(kind,Paint::Panel|Paint::Choice{..}|Paint::Confirm|Paint::PreviewSelected) && node.border!=UiRect::ZERO {node.border=UiRect::ZERO;}
            if matches!(kind,Paint::Hint) {
            let display=if appearance.hints {Display::Flex}else{Display::None};
            if node.display!=display {node.display=display;}
            }
        }
    }
}
fn markers(appearance:Res<Appearance>,menu:Res<super::DialogueMenu>,mut marks:Query<(&Paint,&mut Text,&mut Node)>) {
    for (kind,mut text,mut node) in &mut marks {
        let Paint::ResponseMark(index)=kind else {continue};
        if !appearance.is_changed() && !menu.is_changed() && !text.is_added() {continue;}
        let next=match appearance.theme {
            Theme::Minimal|Theme::ClassicGlass=>String::new(),
            Theme::SplitPanel=>format!("{:02}",index+1),
            Theme::RetroConsole=>if *index==menu.selected {">"}else{"·"}.into(),
            Theme::WristTerminal=>if *index==menu.selected {"•"}else{"·"}.into(),
        };
        let display=if next.is_empty(){Display::None}else{Display::Flex};
        if node.display!=display {node.display=display;}
        if text.0!=next {text.0=next;}
    }
}
pub(super) fn response_mark(parent:&mut ChildSpawnerCommands,index:usize) {
    for part in 0..3 {
        parent.spawn((Paint::ResponseDetail{index,part},bevy::picking::Pickable::IGNORE,BackgroundColor(Color::NONE),
            Node{position_type:PositionType::Absolute,height:px(1.),..default()}));
    }
    parent.spawn((Paint::ResponseMark(index),Text::new(""),TextShadow {offset:Vec2::splat(1.5),color:Color::srgba(0.,0.,0.,0.85)},TextFont{font_size:FontSize::Px(11.),..default()},TextColor(Color::NONE),
        bevy::picking::Pickable::IGNORE,Node{position_type:PositionType::Absolute,left:px(8.),top:px(14.),..default()}));
}

/// Reuse the installed game's scrollbar marker for the wrist-computer cursor.
/// Missing game data/texture keeps the procedural focus cue. No asset is bundled.
fn instrument(
    mut commands:Commands, appearance:Res<Appearance>,
    data:Option<Res<nvf_assets::GameData>>, images:Option<ResMut<Assets<Image>>>,
    formats:Option<Res<bevy::image::CompressedImageFormatSupport>>,
    mut loaded:Local<Option<Option<crate::interface::Picture>>>,
    mut cursors:Query<(Entity,&Paint,&mut BackgroundColor,Option<&ImageNode>)>,
) {
    if appearance.theme==Theme::WristTerminal && loaded.is_none() {
        if let (Some(data),Some(mut images))=(data,images) {
            let mut pictures=crate::interface::Pictures::load(Some(&data));
            *loaded=Some(pictures.picture("interface\\shared\\scrollbar\\vert_marker.dds",Some(&data),&mut images,crate::interface::bc_supported(formats.as_deref())));
        }
    }
    for (entity,kind,mut background,image) in &mut cursors {
        if !matches!(kind,Paint::Focus{..}|Paint::PreviewFocus) {continue;}
        if let Some(Some((handle,rect)))=loaded.as_ref().filter(|_|appearance.theme==Theme::WristTerminal) {
            if image.is_none() {
                let tint=background.0;
                background.0=Color::NONE;
                commands.entity(entity).insert(ImageNode {rect:*rect,color:tint,..ImageNode::new(handle.clone())});
            }
        } else if let Some(image)=image {
            background.0=image.color;
            commands.entity(entity).remove::<ImageNode>();
        }
    }
}

pub(super) fn decoration(parent:&mut ChildSpawnerCommands) {
    // One-pixel insets/rails, original procedural trim; all share unchanged hit geometry.
    for (n,node) in [
        (0,Node{left:px(4.),top:px(3.),width:px(28.),height:px(1.),..default()}),
        (1,Node{right:px(4.),top:px(3.),width:px(28.),height:px(1.),..default()}),
        (2,Node{left:px(3.),bottom:px(4.),width:px(1.),height:px(24.),..default()}),
        (3,Node{right:px(3.),bottom:px(4.),width:px(1.),height:px(24.),..default()}),
        (4,Node{left:px(3.),right:px(3.),top:px(28.),height:px(1.),..default()}),
        (5,Node{left:px(3.),top:px(3.),bottom:px(3.),width:px(1.),..default()}),
        (6,Node{left:px(8.),top:px(6.),width:px(12.),height:px(1.),..default()}),
        (7,Node{left:px(24.),top:px(6.),width:px(6.),height:px(1.),..default()}),
    ] {
        parent.spawn((Paint::Trim(n),bevy::picking::Pickable::IGNORE,Node{position_type:PositionType::Absolute,..node},BackgroundColor(Color::NONE)));
    }
}

fn preview_visibility(appearance:Res<Appearance>,mut roots:Query<&mut Node,With<Preview>>) {
    for mut node in &mut roots {
        node.display=if appearance.theme==Theme::SplitPanel {Display::Flex}else{Display::None};
    }
}

pub(crate) fn is_row(key:&str)->bool { [THEME_KEY,BACKGROUND_KEY,FRAME_KEY,HINTS_KEY,TINT_KEY].contains(&key) }
/// The pause owner supplies its actual clipped note-region Node; never a gameplay panel.
pub(crate) fn preview(parent:&mut ChildSpawnerCommands,node:Node) {
    parent.spawn((Preview,Paint::Panel,node,bevy::picking::Pickable::IGNORE,BackgroundColor(Color::NONE),BorderColor::all(Color::NONE)))
        .with_children(|p| {
            decoration(p);
            p.spawn((Text::new("Dialogue preview"),Paint::Speaker,bevy::picking::Pickable::IGNORE,TextFont{font_size:crate::interface::font_units(16.0),..default()},TextColor(crate::interface::tint(crate::interface::hud_rgb(),255.))));
            p.spawn((PreviewSelection,Paint::PreviewSelected,Node{width:percent(100.),min_height:percent(45.),border:UiRect::all(px(1.)),padding:UiRect::horizontal(px(3.)),..default()},BackgroundColor(Color::NONE),BorderColor::all(Color::NONE),bevy::picking::Pickable::IGNORE))
                .with_children(|p| {p.spawn((Paint::PreviewFocus,bevy::picking::Pickable::IGNORE,Node{position_type:PositionType::Absolute,left:px(1.),top:percent(50.),width:px(2.),height:px(4.),..default()},BackgroundColor(crate::interface::tint(crate::interface::hud_rgb(),255.))));p.spawn((Text::new("Selected response"),Paint::Caption,bevy::picking::Pickable::IGNORE,TextFont{font_size:crate::interface::font_units(16.0),..default()},TextColor(crate::interface::tint(crate::interface::hud_rgb(),255.))));});
        });
}

#[cfg(test)]
mod tests;
