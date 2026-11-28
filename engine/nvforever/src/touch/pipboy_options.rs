//! Independent saved policy; visual suppression never changes input ownership.
use bevy::prelude::*;
use nvf_play::{settings::{self, ChoiceList, Setting, Slider}, touch_safe_area::SafeInsets};

#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub(super) struct Options {
    pub left: bool,
    pub right: bool,
    pub close: bool,
    pub graphics: bool,
    pub wheel: bool,
    pub travel: u16,
}
impl Options {
    pub(super) fn for_platform(android: bool) -> Self {
        Self { left: android, right: android, close: false, graphics: !android, wheel: true, travel: 120 }
    }
}
impl Default for Options {
    fn default() -> Self { Self::for_platform(cfg!(target_os = "android")) }
}
pub(super) const SIDE_KEY:&str="touch-pipboy-dismiss-side-v2";
pub(super) const TRAVEL_KEY:&str="touch-pipboy-dismiss-travel-v1";
const LEFT_KEY:&str="touch-pipboy-dismiss-left";
const RIGHT_KEY:&str="touch-pipboy-dismiss-right";
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Sides { Disabled, Left, Right, Both }
impl Sides {
    fn of(left:bool,right:bool)->Self {match(left,right) {(false,false)=>Self::Disabled,(true,false)=>Self::Left,(false,true)=>Self::Right,(true,true)=>Self::Both}}
    fn name(self)->&'static str {match self {Self::Disabled=>"Disabled",Self::Left=>"Left only",Self::Right=>"Right only",Self::Both=>"Both"}}
    fn parse(s:&str)->Option<Self> {match s.trim().to_ascii_lowercase().as_str() {"disabled"=>Some(Self::Disabled),"left only"=>Some(Self::Left),"right only"=>Some(Self::Right),"both"=>Some(Self::Both),_=>None}}
    fn pair(self)->(bool,bool) {match self {Self::Disabled=>(false,false),Self::Left=>(true,false),Self::Right=>(false,true),Self::Both=>(true,true)}}
}
/// Normalize old and new side rows only in the virtual INI used by the existing
/// parser. Explicit new Disabled beats stale old true values even when the
/// choice loader skips a value equal to its default.
fn migrate_sides(text:&str)->String {
    use fnv_install::{ini_get,ini_set};
    let Some(side)=ini_get(text,settings::SECTION,SIDE_KEY).as_deref().and_then(Sides::parse) else {
        // Absent/malformed choices leave both ordinary boolean loaders alone.
        // Missing or malformed legacy rows retain their existing skip semantics.
        return text.to_owned();
    };
    let (left,right)=side.pair();
    let text=ini_set(text,settings::SECTION,LEFT_KEY,if left {"1"} else {"0"});
    ini_set(&text,settings::SECTION,RIGHT_KEY,if right {"1"} else {"0"})
}

fn choose_sides(world:&mut World,name:&str)->bool {
    let Some(side)=Sides::parse(name) else {return false;};
    let before=*world.resource::<Options>();let (left,right)=side.pair();
    {let mut options=world.resource_mut::<Options>();options.left=left;options.right=right;}
    changed(world,before);true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target { Left, Right, Close }
impl Options {
    pub fn regions(self, size: Vec2, safe: SafeInsets) -> Vec<(Target, Rect)> {
        let values=[safe.left,safe.top,safe.right,safe.bottom];
        if !size.is_finite() || size.min_element()<144.0 || values.iter().any(|v|!v.is_finite() || *v<0.0) { return Vec::new(); }
        let min=Vec2::new(safe.left,safe.top)+Vec2::splat(8.0);
        let max=size-Vec2::new(safe.right,safe.bottom)-Vec2::splat(8.0);
        if (max-min).min_element()<96.0 { return Vec::new(); }
        let y=((min.y+max.y)*0.5).clamp(min.y+48.0,max.y-48.0);
        let mut out=Vec::with_capacity(3);
        if self.left {out.push((Target::Left,Rect::from_center_size(Vec2::new(min.x+24.0,y),Vec2::new(48.0,96.0))));}
        if self.right {out.push((Target::Right,Rect::from_center_size(Vec2::new(max.x-24.0,y),Vec2::new(48.0,96.0))));}
        if self.close {out.push((Target::Close,Rect::from_center_size(Vec2::new((min.x+max.x)*0.5,max.y-24.0),Vec2::splat(48.0))));}
        out
    }
    /// The graphics bit is intentionally absent: paint changes cannot retire input.
    #[cfg(test)]
    pub fn dismissal(self) -> (bool,bool,bool) { (self.left,self.right,self.close) }
    pub fn policy(self) -> (bool,bool,bool,u16) { (self.left,self.right,self.close,self.travel) }
    pub fn travel(self)->f32 {f32::from(self.travel.clamp(48,240))}
    fn space(self) -> nvf_play::pipboy::touch_presentation::DismissalSpace { nvf_play::pipboy::touch_presentation::DismissalSpace::new(self.left,self.right,self.close) }
}
fn changed(world:&mut World,before:Options) {
    let after=*world.resource::<Options>();
    *world.resource_mut::<nvf_play::pipboy::touch_presentation::DismissalSpace>()=after.space();
    if before.policy()!=after.policy() {
        if let Some(mut arm)=world.get_resource_mut::<nvf_play::pipboy::gesture::ArmGesture>() {arm.abort();}
    }
    if before.wheel && !after.wheel {super::pipboy_model::cancel_wheel(world);}
}
fn publish_space(options:Res<Options>,mut space:ResMut<nvf_play::pipboy::touch_presentation::DismissalSpace>) {
    let next=options.space(); if *space!=next {*space=next;}
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<Options>()
        .init_resource::<nvf_play::pipboy::touch_presentation::DismissalSpace>()
        .add_systems(PreUpdate, publish_space.before(nvf_play::touch_safe_area::Publish).before(super::TouchInputSystems));
    macro_rules! switch {
        ($key:literal,$label:literal,$note:literal,$field:ident) => {
            settings::add_setting(app,Setting { key:$key,group:"Controls",label:$label,note:$note,switches:"",
                get:|w|w.resource::<Options>().$field,set:|w,v| {let before=*w.resource::<Options>();w.resource_mut::<Options>().$field=v;changed(w,before);},slider:None });
        };
    }
    switch!("touch-pipboy-dismiss-left","Pip-Boy left dismissal handle","Drag down or diagonally down from the safe left handle to dismiss. The flat menu reserves its space. Reverse before release to keep it open.",left);
    switch!("touch-pipboy-dismiss-right","Pip-Boy right dismissal handle","Drag down or diagonally down from the safe right handle to dismiss. The flat menu reserves its space. Reverse before release to keep it open.",right);
    settings::add_ini_migration(app,SIDE_KEY,migrate_sides);
    // The saved chooser replaces these two legacy rows on the page, not in the INI.
    for key in [LEFT_KEY, RIGHT_KEY] {
        settings::set_shown_when(app, key, SIDE_KEY, |_| false);
    }
    settings::add_choice_list(app,Setting {key:SIDE_KEY,group:"Controls",label:"Pip-Boy downward dismissal",
        note:"Choose where downward drags beside the display lower the arm. Display lists, maps and controls keep their input. Graphics are optional.",
        switches:"",get:|_|false,set:|_,_|{},slider:None},ChoiceList {
        names:|_|[Sides::Disabled,Sides::Left,Sides::Right,Sides::Both].iter().map(|side|side.name().to_owned()).collect(),
        chosen:|w|{let o=w.resource::<Options>();Sides::of(o.left,o.right).name().to_owned()},
        pick:choose_sides,save:|w|{let o=w.resource::<Options>();Sides::of(o.left,o.right).name().to_owned()},load:choose_sides,ask_keep:false,
    });
    settings::add_setting(app,Setting::slider(TRAVEL_KEY,"Controls","Pip-Boy dismissal travel",
        "Drag distance to lower the arm; shorter is more sensitive. Safe remaining screen travel caps it near the bottom. Reverse while held to raise it again.",
        Slider::choice(25,|w|(f32::from(w.resource::<Options>().travel)-48.)/192.,|w,v| {
            let before=*w.resource::<Options>();w.resource_mut::<Options>().travel=(48.+(v.clamp(0.,1.)*24.).round()*8.) as u16;changed(w,before);
        },|v|format!("{:.0} dp",48.+(v*24.).round()*8.))));
    switch!("touch-pipboy-close-button","Pip-Boy close button","Adds an explicit close target. Its touch region stays active when button graphics are hidden.",close);
    switch!("touch-pipboy-button-graphics","Show Pip-Boy button graphics","Shows added button and handle graphics. Hiding them retains identical active touch regions and actions.",graphics);
    switch!("touch-pipboy-model-wheel","Touch Pip-Boy model wheel","Drag the physical wheel on the rendered Pip-Boy to scroll the active list. Mouse, controller and scrollbars stay available.",wheel);
}

/// Explicit full-primary-window UI target; does not change the default UI owner.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct OverlayCanvas<'w,'s> {
    default: bevy::ui::DefaultUiCamera<'w,'s>,
    windows: Query<'w,'s,(Entity,&'static Window),With<bevy::window::PrimaryWindow>>,
    cameras: Query<'w,'s,(Entity,&'static Camera,&'static bevy::camera::RenderTarget)>,
    scale: Option<Res<'w,UiScale>>,
}
#[derive(Clone,Copy,PartialEq)]
pub(super) struct PaintFrame {pub camera:Entity,pub scale:f32}
impl OverlayCanvas<'_, '_> {
    pub fn frame(&self)->Option<PaintFrame> {
        let (window,w)=self.windows.single().ok()?;
        let logical=w.resolution.size();let physical=UVec2::new(w.physical_width(),w.physical_height());
        let scale=self.scale.as_ref().map_or(1.0,|s|s.0);
        if !logical.is_finite() || !scale.is_finite() || scale<=0.0 || !(logical/scale).is_finite() || !(48.0/scale).is_finite() {return None;}
        let valid=|camera:&Camera,target:&bevy::camera::RenderTarget|camera.is_active
            && camera.physical_target_size()==Some(physical)
            && camera.physical_viewport_rect()==Some(URect::from_corners(UVec2::ZERO,physical))
            && camera.logical_viewport_rect()==Some(Rect::from_corners(Vec2::ZERO,logical))
            && matches!(target.normalize(Some(window)),Some(bevy::camera::NormalizedRenderTarget::Window(w)) if w.entity()==window);
        let camera=self.default.get().filter(|e|self.cameras.get(*e).is_ok_and(|(_,c,t)|valid(c,t)))
            .or_else(||self.cameras.iter().filter(|(_,c,t)|valid(c,t)).max_by_key(|(e,c,_)|(c.order,std::cmp::Reverse(e.to_bits()))).map(|(e,_,_)|e))?;
        Some(PaintFrame {camera,scale})
    }
}

#[cfg(test)]
mod tests;
