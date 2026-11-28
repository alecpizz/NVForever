//! Optional direct map input. The wrapper feeds the sole raw touch reader here;
//! the map's real DATA consumer takes typed edits, never synthesized buttons.
use super::*;
use bevy::{ecs::system::SystemParam, input::touch::{TouchInput, TouchPhase}, window::PrimaryWindow};
use std::sync::Arc;

const SLOP: f32 = 12.0;
const MIN_SPAN: f32 = 8.0;
const MAX_EDITS: usize = 16;
const MAX_IDS: usize = 8;

/// Identity of the actual screen presentation, independent of the map's own pan.
/// The publisher supplies only a validated flat crop or visible nearest-part ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Presentation {
    pub safe:crate::touch_safe_area::SafeInsets, pub menu:f32, pub ui:f32, pub size:f32,
}
impl Presentation {
    fn valid(self)->bool {
        crate::touch_safe_area::SafeInsets::from_array([self.safe.left,self.safe.top,self.safe.right,self.safe.bottom]).is_some()
            && [self.menu,self.ui,self.size].into_iter().all(|v|v.is_finite() && v>0.0)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectionStamp {
    pub window: Entity,
    pub logical: Vec2,
    pub physical: UVec2,
    pub density: f32,
    pub image: bevy::asset::AssetId<Image>,
    pub image_scale: f32,
    pub device: bool,
    pub presentation:Presentation,
    pub flat:Option<(Entity,Vec2,Vec2)>,
    pub camera:Option<(Entity,Mat4,Rect)>,
}
impl ProjectionStamp {
    pub fn new(window: Entity, w: &Window, screen: &PipBoyScreen, device: bool, presentation:Presentation) -> Self {
        Self {window, logical:w.resolution.size(), physical:UVec2::new(w.physical_width(),w.physical_height()),
            density:w.scale_factor(), image:screen.image.id(), image_scale:screen.scale, device, presentation,flat:None,camera:None}
    }
    pub fn with_flat(mut self,entity:Entity,size:Vec2,centre:Vec2)->Option<Self> {
        if self.device || !size.is_finite() || size.min_element()<=0.0 || !centre.is_finite() {return None;}
        self.flat=Some((entity,size,centre));Some(self)
    }
    pub fn with_camera(mut self,entity:Entity,camera:&Camera,target:&bevy::camera::RenderTarget)->Option<Self> {
        let expected=bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(self.window));
        if target.normalize(Some(self.window))!=expected.normalize(Some(self.window)) {return None;}
        let viewport=camera.logical_viewport_rect()?;
        if !self.device || !camera.is_active || !camera.computed.clip_from_view.is_finite()
            || !viewport.min.is_finite() || !viewport.max.is_finite() || viewport.size().min_element()<=0.0 {return None;}
        self.camera=Some((entity,camera.computed.clip_from_view,viewport));Some(self)
    }
    fn valid(self) -> bool {
        self.presentation.valid() && self.logical.is_finite() && self.logical.min_element()>0.0 && self.physical.min_element()>0
            && self.density.is_finite() && self.density>0.0 && self.image_scale.is_finite() && self.image_scale>0.0
            && if self.device {self.camera.is_some()&&self.flat.is_none()}else{self.flat.is_some()&&self.camera.is_none()}
    }
}
/// One immutable last-painted mapper. A device closure moves the already computed
/// posed meshes into this Arc; it need not clone any mesh/triangle arrays.
#[derive(Clone)]
pub struct Projection {
    stamp: ProjectionStamp,
    map: Arc<dyn Fn(Vec2)->Option<Vec2> + Send + Sync>,
}
impl Projection {
    pub fn new(stamp: ProjectionStamp, map: impl Fn(Vec2)->Option<Vec2> + Send + Sync + 'static) -> Option<Self> {
        stamp.valid().then(||Self {stamp,map:Arc::new(map)})
    }
    fn point(&self, raw: Vec2) -> Option<Vec2> {
        (raw.is_finite() && Rect::from_corners(Vec2::ZERO,self.stamp.logical).contains(raw))
            .then(||(self.map)(raw)).flatten().filter(|p|p.is_finite())
    }
}
#[derive(Clone, Copy, PartialEq)]
struct SurfaceKey {tab:usize, window:Rect, space:u32, local:Option<([f32;2],Vec2)>, usable:Vec2}
struct Surface<'a> {key:SurfaceKey, view:MapView, mag:f32, map:&'a MapShown}
impl Surface<'_> {
    fn marker(&self, point:Vec2)->Option<u32> {
        let at=self.view.pointer(Some(point))?;
        let parts=self.map.world.as_ref()?;
        let marks=parts.rows.as_deref().unwrap_or(self.map.marks());
        let place=|m:&MapMark|m.uv.map(|uv|self.view.to_window(uv)).unwrap_or(m.at);
        nearest(marks.iter().map(place),at).map(|i|marks[i].form)
    }
}
fn surface(st:&DataState,tab:usize)->Option<Surface<'_>> {
    if tab>WORLD || st.popup.is_some() {return None;}
    let shown=if st.map_window.is_some_and(|w|w.0==tab) {st.last_view.as_ref()} else {st.shown.as_ref()}.filter(|v|v.tab==tab)?;
    let Body::Map(map)=&shown.body else {return None};
    let parts=map.world.as_ref()?;
    let view=parts.view.with_window(map_window(st,tab));
    let mag=st.magnify[tab].max(ZOOM.0);
    let space=st.spot[tab]?.space;
    if !view.window.min.is_finite() || !view.window.max.is_finite() || view.extent().min_element()<=0.0
        || !mag.is_finite() || parts.mag!=mag {return None;}
    let local=view.local.map(|p|(p.origin,p.extent));
    let usable=if local.is_none() {Vec2::from(st.maps.get(&space)?.as_ref()?.usable)}else{Vec2::ZERO};
    Some(Surface {key:SurfaceKey{tab,window:view.window,space,local,usable},view,mag,map})
}
#[derive(Clone, Copy, PartialEq)]
struct Context {
    projection:ProjectionStamp, surface:SurfaceKey, layout_epoch:u64,
}
#[derive(Clone, Copy)]
struct Finger {id:u64, origin:Vec2, at:Vec2}
#[derive(Clone, Copy)]
enum Edit {Transform {from:Vec2,to:Vec2,ratio:f32}, Tap {point:Vec2,marker:Option<u32>}}
struct Batch {context:Context, base:MapView, mag:f32, edits:Vec<Edit>, pointer:Option<Vec2>, ids:Vec<u64>}
/// Absent leaves desktop input untouched. Contact/queue limits are fixed and
/// cancellation drops pending edits as well as the gesture's candidate tap.
#[derive(Resource, Default)]
pub struct TouchMap {
    projection:Option<Projection>, context:Option<Context>, fingers:[Option<Finger>;2],
    blocked:Vec<u64>, edits:Vec<Edit>, base:Option<(MapView,f32)>, ids:Vec<u64>,
    pointer:Option<Vec2>, moved:bool, multiple:bool, marker:Option<u32>, drain:bool, overflow:bool,
}
impl TouchMap {
    pub fn publish(&mut self, projection:Option<Projection>) {
        if projection.is_none() || self.projection.as_ref().zip(projection.as_ref()).is_some_and(|(a,b)|a.stamp!=b.stamp) {
            self.cancel();self.context=None;
        }
        self.projection=projection;
    }
    /// Read-only calibration of the exact active projector, also used by source
    /// integration tests to verify flat/device mapping without sending actions.
    pub fn projected_point(&self,raw:Vec2)->Option<Vec2> {self.projection.as_ref()?.point(raw)}
    pub fn captures(&self)->bool {self.overflow || self.drain || !self.blocked.is_empty() || self.fingers.iter().any(Option::is_some)}
    pub fn cancel(&mut self) {
        let ids=self.fingers.map(|f|f.map(|f|f.id));
        for id in ids.into_iter().flatten() {
            if !self.blocked.contains(&id) {
                if self.blocked.len()>=MAX_IDS {self.overflow=true;self.blocked.clear();break;}
                self.blocked.push(id);
            }
        }
        self.fingers=[None,None];self.edits.clear();self.base=None;self.ids.clear();self.pointer=None;
        self.moved=false;self.multiple=false;self.marker=None;
    }
    fn overflow(&mut self) {
        self.cancel();self.overflow=true;self.blocked.clear();self.drain=true;
    }
    fn remember(&mut self,id:u64)->bool {
        if self.ids.contains(&id) {return true;}
        if self.ids.len()>=MAX_IDS {self.overflow();return false;}
        self.ids.push(id);true
    }
    fn push(&mut self, edit:Edit, view:MapView,mag:f32)->bool {
        if self.edits.len()>=MAX_EDITS {self.overflow();return false;}
        self.base.get_or_insert((view,mag));self.edits.push(edit);true
    }
    fn take(&mut self)->Option<Batch> {
        let context=self.context?;
        let (base,mag)=self.base.take()?;
        Some(Batch {context,base,mag,edits:std::mem::take(&mut self.edits),pointer:self.pointer,ids:std::mem::take(&mut self.ids)})
    }
}
/// The real game owns script/side-menu admission. Keep this read out of Gate:
/// DATA already mutably borrows ScriptState through its existing Game parameter.
/// Parent raw ingress only reads it in Bridge; DATA repeats it through that borrow.
pub(super) fn source_allows(script:Option<&ScriptState>)->bool {
    script.and_then(|s|s.rt.as_ref()).is_none_or(|rt|
        !crate::tiles::stats_menu::modal_over(rt.game.top_menu())
        && !crate::tiles::stats_menu::modal_over(rt.game.side_menu()))
}
/// Shared late admission; desktop fixtures need none of these optional resources.
#[derive(SystemParam)]
pub struct Gate<'w,'s> {
    admission:super::super::touch_items::InputAdmission<'w>,
    physical:Option<Res<'w,super::super::touch_scroll::PhysicalPriority>>,
    windows:Query<'w,'s,(Entity,&'static Window),With<PrimaryWindow>>,
    safe:Option<Res<'w,crate::touch_safe_area::SafeInsets>>, ui:Option<Res<'w,UiScale>>,
    sizing:Option<Res<'w,super::super::touch_presentation::PipBoySize>>,
    dismissal:Option<Res<'w,super::super::touch_presentation::DismissalSpace>>,
    creator:Option<Res<'w,crate::creator::CreatorMenu>>,
    paused:Option<Res<'w,crate::pause::Paused>>, location:Option<Res<'w,crate::Location>>,
    touches:Option<Res<'w,bevy::input::touch::Touches>>,
    flat:Query<'w,'s,(Entity,&'static ComputedNode,&'static UiGlobalTransform),With<super::super::FlatScreen>>,
    cameras:Query<'w,'s,(Entity,&'static Camera,&'static bevy::camera::RenderTarget),With<crate::PlayerCamera>>,
}
impl Gate<'_,'_> {
    fn stamp(&self,pip:&PipBoy,screen:&PipBoyScreen)->Option<ProjectionStamp> {
        let (window_id,window)=self.windows.single().ok()?;
        if !window.focused || pip.phase!=super::super::Phase::Up || pip.page!=Page::Data || pip.tab()>WORLD
            || self.paused.as_ref().is_some_and(|p|p.0) || !self.admission.allows()
            || self.creator.as_ref().is_some_and(|c|c.is_open())
            || self.physical.as_ref().is_some_and(|p|p.active) {return None;}
        let device=pip.frame==super::super::Frame::Device && screen.on_model;
        let safe=self.safe.as_deref().copied().unwrap_or_default();
        let safe=if device {safe} else {self.dismissal.as_deref().copied().unwrap_or_default().padding(safe,Some(window.resolution.size()))};
        let presentation=Presentation {
            safe, menu:crate::interface::unit_scale(),
            ui:self.ui.as_deref().map_or(1.0,|v|v.0),
            size:self.sizing.as_deref().map_or(1.0,super::super::touch_presentation::PipBoySize::ratio),
        };
        let stamp=ProjectionStamp::new(window_id,window,screen,device,presentation);
        let stamp=if device {
            let (entity,camera,target)=self.cameras.single().ok()?;stamp.with_camera(entity,camera,target)?
        } else {
            let (entity,node,at)=self.flat.single().ok()?;stamp.with_flat(entity,node.size(),at.translation)?
        };
        stamp.valid().then_some(stamp)
    }
    fn space(&self,tab:usize)->Option<u32> {self.location.as_ref().map(|l|if tab==LOCAL {l.interior.unwrap_or(l.world)}else{l.world})}
}
#[derive(SystemParam)]
pub struct Bridge<'w,'s> {
    state:ResMut<'w,TouchMap>, pip:Option<Res<'w,PipBoy>>, screen:Option<Res<'w,PipBoyScreen>>,
    data:Option<Res<'w,DataState>>, gate:Gate<'w,'s>, script:Option<Res<'w,ScriptState>>,
}
impl Bridge<'_,'_> {
    /// Called before the raw batch. `allowed` includes wrapper lifecycle, UI,
    /// current posed-screen eligibility and physical keyboard ownership.
    pub fn prepare(&mut self,allowed:bool,layout_epoch:u64) {
        self.state.drain=false;
        // The existing InputPlugin is the neutral witness; no speculative ID
        // count or synthetic release may reopen an overflowed gesture.
        let neutral=self.gate.touches.as_ref().is_some_and(|touches|touches.iter().next().is_none());
        if neutral {self.state.overflow=false;self.state.blocked.clear();}
        if self.state.overflow {self.state.context=None;return;}
        let next=(|| {
            let (pip,screen,data)=(self.pip.as_deref()?,self.screen.as_deref()?,self.data.as_deref()?);
            if !allowed || !source_allows(self.script.as_deref()) {return None;}
            let stamp=self.gate.stamp(pip,screen)?;
            if self.state.projection.as_ref()?.stamp!=stamp {return None;}
            let surface=surface(data,pip.tab())?;
            if self.gate.space(pip.tab())!=Some(surface.key.space) {return None;}
            Some(Context {projection:stamp,surface:surface.key,layout_epoch})
        })();
        if self.state.context!=next {self.state.cancel();}
        if next.is_none() {self.state.projection=None;}
        self.state.context=next;
    }
    pub fn captures(&self)->bool {self.state.captures()}
    pub fn cancel(&mut self) {self.state.cancel();self.state.context=None;}
    /// Returns owned/canceled tails too. The caller must stop ordinary touch
    /// routes on true, even when the map context just became unavailable.
    pub fn event(&mut self,event:&TouchInput,eligible:bool)->bool {
        if self.state.overflow {return true;}
        if !self.state.blocked.is_empty() {
            if event.phase==TouchPhase::Started {self.state.overflow();return true;}
            if matches!(event.phase,TouchPhase::Ended|TouchPhase::Canceled) {self.state.blocked.retain(|id|*id!=event.id);}
            return true;
        }
        if self.state.drain {return true;}
        let owned=self.state.fingers.iter().flatten().any(|f|f.id==event.id);
        let Some(context)=self.state.context else {return owned;};
        if event.window!=context.projection.window || !self.gate.admission.allows_contact(Some(event.id)) {
            if owned {self.state.cancel();return true;}return false;
        }
        let Some(projection)=self.state.projection.clone() else {self.state.cancel();return owned;};
        let Some(surface)=self.data.as_deref().and_then(|d|surface(d,context.surface.tab)) else {self.state.cancel();return owned;};
        if surface.key!=context.surface {self.state.cancel();return owned;}
        if event.phase==TouchPhase::Started {
            if owned {self.state.cancel();return true;}
            let at=projection.point(event.position).filter(|p|surface.view.window.contains(*p));
            if !eligible || at.is_none() || on_button(self.data.as_deref().unwrap(),context.surface.tab,at) {
                if self.state.captures() {self.state.cancel();self.state.drain=true;return true;}return false;
            }
            let Some(index)=self.state.fingers.iter().position(Option::is_none) else {self.state.overflow();return true;};
            if self.state.fingers.iter().all(Option::is_none) {
                self.state.ids.clear();self.state.moved=false;self.state.multiple=false;self.state.marker=surface.marker(at.unwrap());
                self.state.pointer=at;
            } else {self.state.multiple=true;self.state.moved=true;}
            if !self.state.remember(event.id) {return true;}
            self.state.fingers[index]=Some(Finger{id:event.id,origin:event.position,at:event.position});
            // Cursor-only batch, with no action edge, follows a held finger too.
            self.state.base.get_or_insert((surface.view,surface.mag));
            return true;
        }
        let Some(index)=self.state.fingers.iter().position(|f|f.is_some_and(|f|f.id==event.id)) else {return false;};
        // A previous frame consumed the queue's IDs. Re-add the owner on every
        // new sample, including Ended before removing its last Finger, so a
        // late per-contact quarantine still invalidates this batch.
        if !self.state.remember(event.id) {return true;}
        if event.phase==TouchPhase::Canceled || !event.position.is_finite() {
            self.state.cancel();self.state.blocked.retain(|id|*id!=event.id);self.state.drain=true;return true;
        }
        let finger=self.state.fingers[index].unwrap();
        let before_raw=if self.state.moved {finger.at}else{finger.origin};
        let before=projection.point(before_raw);let after=projection.point(event.position);
        let (Some(before),Some(after))=(before,after) else {self.state.cancel();self.state.drain=true;return true;};
        if !surface.view.window.contains(after) {self.state.cancel();self.state.drain=true;return true;}
        if let Some(other)=self.state.fingers[1-index] {
            let other_at=projection.point(other.at);
            let Some(other_at)=other_at else {self.state.cancel();return true;};
            let old_span=before.distance(other_at);let new_span=after.distance(other_at);
            if finger.at.distance(other.at)<MIN_SPAN || event.position.distance(other.at)<MIN_SPAN
                || old_span<=0.0 || !new_span.is_finite() {self.state.cancel();self.state.drain=true;return true;}
            if !self.state.push(Edit::Transform {from:(before+other_at)*0.5,to:(after+other_at)*0.5,ratio:new_span/old_span},surface.view,surface.mag) {return true;}
            self.state.pointer=Some((after+other_at)*0.5);
        } else if self.state.moved || event.position.distance(finger.origin)>SLOP {
            self.state.moved=true;
            if !self.state.push(Edit::Transform{from:before,to:after,ratio:1.0},surface.view,surface.mag) {return true;}
            self.state.pointer=Some(after);
        } else {self.state.pointer=Some(after);}
        if let Some(f)=self.state.fingers[index].as_mut() {f.at=event.position;}
        if event.phase==TouchPhase::Ended {
            if !self.state.moved && !self.state.multiple {
                let marker=self.state.marker;
                if marker==surface.marker(after) && !self.state.push(Edit::Tap{point:after,marker},surface.view,surface.mag) {return true;}
            }
            self.state.fingers[index]=None;
            if self.state.fingers.iter().all(Option::is_none) {self.state.drain=true;}
        }
        true
    }
}

/// Called by the real DATA step before hardware input. Late admission and current
/// model geometry are rechecked; never apply old edits to another map/tab/window.
pub(super) fn take(st:&mut DataState,tab:usize,screen:Option<&PipBoyScreen>,input:&mut Input,allowed:bool)->Option<(Vec2,Option<Option<u32>>)> {
    let hardware=input.physical.as_ref().is_some_and(|p|p.active)
        || [MouseButton::Left,MouseButton::Right,MouseButton::Middle].into_iter().any(|b|input.buttons.pressed(b)||input.buttons.just_pressed(b)||input.buttons.just_released(b))
        || !input.wheel.is_empty()
        || input.keys.any_just_pressed([KeyCode::Enter,KeyCode::NumpadEnter,KeyCode::Space,KeyCode::KeyX,KeyCode::KeyE]);
    let stamp=input.touch_pip.as_deref().zip(screen).and_then(|(pip,screen)|input.map_gate.stamp(pip,screen));
    let state=input.touch_map.as_deref_mut()?;
    if state.overflow || !allowed || hardware || input.map_overlay.as_ref().is_some_and(|overlay| overlay.0) || !input.map_gate.admission.allows()
        || state.fingers.iter().flatten().any(|f|!input.map_gate.admission.allows_contact(Some(f.id))) {
        state.cancel();return None;
    }
    let mut batch=state.take()?;
    if stamp!=Some(batch.context.projection) || input.map_gate.space(tab)!=Some(batch.context.surface.space)
        || batch.ids.iter().any(|id|!input.map_gate.admission.allows_contact(Some(*id))) {
        state.cancel();return None;
    }
    let Some(current)=surface(st,tab) else {state.cancel();return None;};
    if current.key!=batch.context.surface || current.view!=batch.base || current.mag!=batch.mag
        || screen.is_none_or(|s|s.image.id()!=batch.context.projection.image || s.scale!=batch.context.projection.image_scale) {
        state.cancel();return None;
    }
    let mut view=current.view;let mut mag=current.mag;let mut tap=None;let mut changed=None;
    let current_centre=view.local.map_or(view.centre,|p|p.centre);
    if st.centre[tab]!=Some(current_centre) {state.cancel();return None;}
    for edit in batch.edits.drain(..) {
        match edit {
            Edit::Transform{from,to,ratio}=>{
                let Some((centre,next))=view.touch_transform(mag,from-view.window.min,to-view.window.min,ratio) else {state.cancel();return None;};
                changed=Some(centre);mag=next;
                view=if let Some(local)=view.local {MapView::local(view.window,view::LocalProjection{centre,mag:next,..local})}
                    else {MapView::world(view.window,centre,batch.context.surface.usable,next)};
            }
            Edit::Tap{point,marker}=>{state.pointer=Some(point);tap=Some(marker);}
        }
    }
    // Reuse bounded storage on steady motion frames instead of allocating a
    // new queue each frame. No completed IDs or edits remain in the storage.
    batch.ids.clear();state.ids=batch.ids;
    state.edits=batch.edits;
    if let Some(centre)=changed {
        st.centre[tab]=Some(centre);st.magnify[tab]=mag;st.press=None;st.drag_from=None;
    }
    let pointer=state.pointer.or(batch.pointer)?;
    // Keep held idle frames cursor-only without repeating pan, pinch or tap.
    if state.fingers.iter().any(Option::is_some) {state.base=Some((view,mag));}
    Some((pointer,tap))
}
pub(super) fn finish(st:&mut DataState,view:&View,tap:Option<Option<u32>>) {
    let Some(expected)=tap else {return;};
    let Body::Map(map)=&view.body else {return;};
    let picked=map.marks.iter().find(|m|m.highlighted).map(|m|m.form);
    if view.tab==WORLD {
        st.travel_marker=picked;
        if expected.is_some() && picked==expected {st.travel_pending=picked;}
    }
}



#[cfg(test)]
pub(crate) mod tests;
