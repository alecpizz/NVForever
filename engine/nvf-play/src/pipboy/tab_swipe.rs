//! Navigation shares the existing projected pointer stream; it never reads raw events.
#[cfg(test)]
#[path="tab_swipe/tests.rs"]
mod tests;
use super::{Page,PipBoy,PipBoyInput,Phase};
use bevy::{input::touch::TouchPhase,prelude::*};
pub const KEY: &str="touch-pipboy-tab-swipes";
const PLAYER:usize=crate::menu::model::Side::Player as usize;
#[derive(Resource)]
#[cfg_attr(not(target_os="android"),derive(Default))]
pub struct Options {pub enabled:bool}
#[cfg(target_os="android")]
impl Default for Options {fn default()->Self {Self {enabled:true}}}
pub(super) fn install(app:&mut App) {
    app.init_resource::<Swipe>().init_resource::<Options>()
        .add_systems(Update,dispatch.in_set(crate::PlaySet::Ui).in_set(Systems::Dispatch).before(super::apply_input))
        .add_systems(Update,restore_history.in_set(crate::PlaySet::Ui).in_set(Systems::Restore).after(super::apply_input).before(super::items::keys));
    #[cfg(any(target_os = "android", test))]
    crate::settings::add_setting(app,crate::settings::Setting {
        key:KEY,group:crate::pause::GROUP_MENUS,label:"Pip-Boy title/tab swipes",
        note:"Swipe the title to change sections; swipe the tab names to change subtabs. Body swipes scroll or drag items.",
        switches:"",get:|w|w.resource::<Options>().enabled,set:|w,v|w.resource_mut::<Options>().enabled=v,slider:None,
    });
}
#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub enum Systems {Dispatch,Restore}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind { Sections, Subtabs }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Key { Native(fnv_ui::TileId), Handbuilt(Entity) }
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct Region { pub key:Key, pub kind:Kind, pub rect:Rect }
/// Marks only painted navigation controls. Content rows and scrollbars never get it.
#[derive(Component,Clone,Copy)]
pub struct Strip(pub Kind);
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Axis { Pending, Horizontal, Rejected }
#[derive(Clone,Copy)]
struct Contact {
    region:Region, origin:Vec2, page:Page, tab:usize, frame:super::Frame, axis:Axis,
}
#[derive(Resource,Default)]
pub struct Swipe {
    contact:Option<Contact>, enabled:Option<bool>, queued:Option<Request>,
    bookmarks:[Option<Bookmark>;5], restore:Option<(Page,usize)>,
}
#[derive(Clone,Copy)]
struct Request {input:PipBoyInput,page:Page,tab:usize,frame:super::Frame,image:bevy::asset::AssetId<Image>,scale:u32,id:Option<u64>}
#[derive(Clone,Copy)]
struct Bookmark {row:crate::menu::model::RowKey,scroll:usize,native_scroll:Option<f64>}
#[derive(Default,Debug)]
pub struct Outcome { pub consume:bool, pub request:Option<PipBoyInput> }
impl Swipe {
    pub fn cancel(&mut self) { self.contact=None; self.queued=None; }
    /// `logical` is the original window position, never zoomed canvas coordinates.
    pub fn observe(&mut self,pip:&PipBoy,phase:TouchPhase,logical:Vec2,canvas:Option<Vec2>,age_ms:u64,regions:&[Region])->Outcome {
        if phase==TouchPhase::Started {
            self.cancel();
            if pip.phase==Phase::Up && logical.is_finite() {
                if let Some(region)=canvas.and_then(|p|regions.iter().find(|r|r.rect.contains(p))).copied() {
                    self.contact=Some(Contact {region,origin:logical,page:pip.page,tab:pip.tab(),frame:pip.frame,axis:Axis::Pending});
                }
            }
            // Native taps remain release-only; Bevy press may start, then gets Cancel
            // if this becomes a swipe. Starting a gesture must not switch a tab.
            return Outcome::default();
        }
        let Some(mut c)=self.contact else {return Outcome::default()};
        let valid=canvas.is_some() && pip.phase==Phase::Up && pip.page==c.page && pip.tab()==c.tab && pip.frame==c.frame
            && logical.is_finite() && regions.contains(&c.region);
        if !valid || phase==TouchPhase::Canceled {
            self.cancel();
            return Outcome {consume:true,request:None};
        }
        let delta=logical-c.origin;
        if c.axis==Axis::Pending {
            if delta.length()>12.0 {
                c.axis=if delta.x.abs()>delta.y.abs()*1.25 {Axis::Horizontal}else{Axis::Rejected};
            } else if age_ms>=450 {c.axis=Axis::Rejected;}
        }
        self.contact=Some(c);
        let ended=phase==TouchPhase::Ended;
        let request=if ended && c.axis==Axis::Horizontal && delta.x.abs()>=48.0 {
            let step=if delta.x<0.0 {1}else{-1};
            Some(match c.region.kind {
                Kind::Subtabs=>PipBoyInput::StepTab(step),
                Kind::Sections=>{
                    let index=Page::ALL.iter().position(|p|*p==pip.page).unwrap_or(0) as i32;
                    PipBoyInput::Page(Page::ALL[(index+step).rem_euclid(Page::ALL.len() as i32) as usize])
                },
            })
        }else{None};
        if ended {self.cancel();}
        Outcome {consume:c.axis!=Axis::Pending,request}
    }
}

/// Use the exact visible/clipped text pieces retained by the native menu painter.
pub fn native_region(piece:&crate::tiles::draw::Piece,kind:Kind)->Option<Region> {
    if !matches!(piece.look,crate::tiles::draw::Look::Text{..}) || !piece.colour[3].is_finite() || piece.colour[3]<=0.0 || piece.turn.is_some() {return None}
    if ![piece.rect.left,piece.rect.top,piece.rect.width,piece.rect.height].into_iter().all(f64::is_finite) || piece.rect.width<=0.0 || piece.rect.height<=0.0 {return None}
    let mut rect=Rect::from_corners(Vec2::new(piece.rect.left as f32,piece.rect.top as f32),Vec2::new((piece.rect.left+piece.rect.width) as f32,(piece.rect.top+piece.rect.height) as f32));
    if let Some(clip)=piece.clip {
        if ![clip.left,clip.top,clip.width,clip.height].into_iter().all(f64::is_finite) || clip.width<=0.0 || clip.height<=0.0 {return None}
        rect=rect.intersect(Rect::from_corners(Vec2::new(clip.left as f32,clip.top as f32),Vec2::new((clip.left+clip.width) as f32,(clip.top+clip.height) as f32)));
    }
    (rect.min.is_finite() && rect.max.is_finite() && rect.width()>0.0 && rect.height()>0.0).then_some(Region {key:Key::Native(piece.tile),kind,rect})
}

/// The pointer coordinator calls this after projection and ordinary input admission.
/// It shares the existing touch owner, geometry and output message route.
type NavigationStrips<'w,'s> = Query<'w,'s,(Entity,&'static Strip,&'static ComputedNode,&'static UiGlobalTransform,Option<&'static bevy::ui::CalculatedClip>,Option<&'static InheritedVisibility>)>;
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Navigation<'w,'s> {
    state:Option<ResMut<'w,Swipe>>, options:Option<Res<'w,Options>>,
    nodes:NavigationStrips<'w,'s>,
}
impl Navigation<'_,'_> {
    pub(super) fn cancel(&mut self) {if let Some(state)=self.state.as_deref_mut() {state.cancel();}}
    pub(super) fn observe(&mut self,pip:&PipBoy,screen:&super::PipBoyScreen,point:&super::touch_items::Point,id:Option<u64>,at:Option<Vec2>,mut regions:Vec<Region>)->bool {
        let (Some(state),Some(options))=(self.state.as_deref_mut(),self.options.as_deref()) else {return false};
        let changed=state.enabled.is_some_and(|old|old!=options.enabled);
        let pending=state.contact.is_some();
        state.enabled=Some(options.enabled);
        if changed {state.cancel();if pending {return true}}
        if !options.enabled {state.cancel();return false}
        for (entity,strip,node,transform,clip,visible) in &self.nodes {
            if visible.is_some_and(|v|!v.get()) || !node.size().is_finite() || node.size().min_element()<=0.0 {continue}
            let mut rect=Rect::from_center_size(transform.transform_point2(Vec2::ZERO),node.size());
            if let Some(clip)=clip {
                // CalculatedClip is a transformed clip stack in Bevy0.20. Exact
                // point admission includes FullyClipped and rotated ancestors.
                if at.is_none_or(|p|!clip.contains_point(p*screen.scale)) {continue}
                let Some(clips)=clip.rects() else {continue};
                let mut supported=true;
                for entry in clips {
                    let transform=entry.world_to_clip_local;
                    // These flat menu strips are axis-aligned. Fail closed for a
                    // rotated clip rather than broadening its scissor to an AABB.
                    if !transform.is_finite() || transform.matrix2.determinant()==0.0
                        || transform.matrix2.x_axis.y!=0.0 || transform.matrix2.y_axis.x!=0.0 {supported=false;break}
                    let inverse=transform.inverse();
                    let a=inverse.transform_point2(entry.rect.min);let b=inverse.transform_point2(entry.rect.max);
                    rect=rect.intersect(Rect::from_corners(a.min(b),a.max(b)));
                }
                if !supported {continue}
            }
            let scale=screen.scale;
            if !scale.is_finite() || scale<=0.0 {continue}
            rect=Rect::from_corners(rect.min/scale,rect.max/scale);
            if rect.min.is_finite() && rect.max.is_finite() && rect.width()>0.0 && rect.height()>0.0 {
                regions.push(Region {key:Key::Handbuilt(entity),kind:strip.0,rect});
            }
        }
        let outcome=state.observe(pip,point.phase,point.position,at,point.age_ms,&regions);
        if let Some(input)=outcome.request {
            state.queued=Some(Request {input,page:pip.page,tab:pip.tab(),frame:pip.frame,image:screen.image.id(),scale:screen.scale.to_bits(),id});
        }
        outcome.consume
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch(mut state:ResMut<Swipe>,options:Res<Options>,pip:Res<PipBoy>,screen:Option<Res<super::PipBoyScreen>>,
    admission:super::touch_items::Admission,routing:super::touch_scroll::Routing,
    priority:Option<Res<super::touch_scroll::PhysicalPriority>>,keys:Option<Res<ButtonInput<KeyCode>>>,mut out:MessageWriter<PipBoyInput>) {
    let Some(request)=state.queued.take() else {return};
    let stable=screen.is_some_and(|s|s.image.id()==request.image && s.scale.to_bits()==request.scale);
    if !options.enabled || !stable || pip.phase!=Phase::Up || pip.page!=request.page || pip.tab()!=request.tab || pip.frame!=request.frame
        || !admission.allows_contact(request.id) || priority.is_some_and(|p|p.active)
        || keys.is_some_and(|k|k.any_just_pressed([KeyCode::F1,KeyCode::F2,KeyCode::F3,KeyCode::ArrowLeft,KeyCode::ArrowRight,KeyCode::KeyA,KeyCode::KeyD,KeyCode::Tab,KeyCode::Escape]))
        || !routing.allows_navigation(&pip) {return}
    if let Some(model)=routing.inventory_model().filter(|m|request.page==Page::Items && m.mode==crate::menu::model::Mode::Inventory && m.folder.is_none()) {
        if let Some(index)=super::items::TABS.iter().position(|c|*c==model.category[PLAYER]) {
            if let Some(row)=crate::menu::model::RowKey::at(&model.lists[PLAYER].rows,model.selected[PLAYER]) {
                state.bookmarks[index]=Some(Bookmark {row,scroll:routing.handbuilt_inventory_scroll(&pip).unwrap_or(model.scroll[PLAYER]),native_scroll:routing.native_inventory_scroll(&pip)});
            }
        }
    }
    let (page,tab)=match request.input {
        PipBoyInput::Page(page)=>(page,if page==pip.page {pip.tab()}else{pip.tabs[page.index()]}),
        PipBoyInput::Tab(tab)=>(pip.page,tab.min(4)),
        PipBoyInput::StepTab(step)=>(pip.page,(pip.tab() as i32+step).rem_euclid(5) as usize),
    };
    state.restore=Some((page,tab));
    out.write(request.input);
}
fn restore_history(mut state:ResMut<Swipe>,pip:Res<PipBoy>,active:Option<ResMut<crate::menu::ActiveMenu>>,fresh:Option<Res<crate::menu::FreshMenu>>,mut tiles:Option<ResMut<crate::tiles::inventory_menu::InventoryTiles>>) {
    if pip.phase!=Phase::Up || fresh.is_some_and(|f|f.0) {
        state.bookmarks=[None;5]; state.restore=None; state.cancel();if let Some(t)=tiles.as_deref_mut(){t.clear_navigation_scroll();}return
    }
    let Some((page,tab))=state.restore else {return};
    if page!=pip.page || tab!=pip.tab() {state.restore=None;return}
    if page!=Page::Items {state.restore=None;return}
    let Some(mut active)=active else {state.restore=None;return};
    let Some(model)=active.0.as_mut().filter(|m|m.mode==crate::menu::model::Mode::Inventory && m.folder.is_none()) else {state.restore=None;return};
    if model.category[PLAYER]!=super::items::TABS[tab] {return} // Wait for actual SetCategory.
    if let Some(bookmark)=state.bookmarks[tab] {
        let rows=&model.lists[PLAYER].rows;
        if let Some(index)=(0..rows.len()).find(|i|crate::menu::model::RowKey::at(rows,*i)==Some(bookmark.row)) {
            model.selected[PLAYER]=index;
            model.scroll[PLAYER]=bookmark.scroll.min(rows.len().saturating_sub(1));
            model.pointer_pick=true;
            if let Some(mut tiles)=tiles {
                if let Some(value)=bookmark.native_scroll {tiles.queue_navigation_scroll(model.category[PLAYER],bookmark.row,value);}
            }
        }
    }
    state.restore=None;
}
