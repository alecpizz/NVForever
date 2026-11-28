//! Finger input over the ordinary barter presentation. No replacement UI is drawn.
//! The wrapper owns raw input/provenance. Taps wait for End; a held quantity
//! meter proposes live Amount only. Both resolve one-shot envelopes inside the
//! ordinary MenuInput consumer.
use super::{model::{Category, Command, MenuModel, Mode, Quantity, RowKey, Side, Trade},
    ActiveMenu, CmdButton, MenuInput, RowButton, AmountMeter};
use bevy::{ecs::system::SystemParam, input::touch::{TouchInput, TouchPhase},
    prelude::*, ui::{CalculatedClip, ComputedUiTargetCamera, FocusPolicy, UiGlobalTransform, UiStack}};

pub use super::barter_touch::Frame;
const SLOP: f32 = 12.0;
const HEADER_SWIPE: f32 = 28.0;
const MAX_IDS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target { Row(Side, usize), Command(Command), Meter(f32), Pane(Side), CategoryHeader(Side) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source { Node(Entity), Tile(fnv_ui::TileId, fnv_ui::TileId), Direct(fnv_ui::TileId, fnv_ui::TileId) }
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit { pub source: Source, pub target: Target, pub pane: Option<Side>, pub row_step:f32 }

#[derive(Clone, Copy, PartialEq)]
struct Stamp { window: Entity, camera: Entity, size: Vec2,
    safe: crate::touch_safe_area::SafeInsets, density: f32, ui_scale: f32, menu_units:f32,text_size:f32 }
impl Stamp {
    fn of(f:Frame,text_size:f32)->Self { Self { window:f.window, camera:f.camera, size:f.size,
        safe:f.safe, density:f.density, ui_scale:f.ui_scale, menu_units:crate::interface::unit_scale(),text_size } }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct RowStamp { side: Side, index: usize, key: RowKey, count: i32,
    bartered: bool, price: Option<u32> }
impl RowStamp {
    fn of(m: &MenuModel, side: Side, index: usize) -> Option<Self> {
        let r=m.list(side).rows.get(index)?;
        Some(Self { side,index,key:RowKey::at(&m.list(side).rows,index)?,count:r.count,
            bartered:r.bartered,price:r.price.map(f32::to_bits) })
    }
    fn matches(&self,m:&MenuModel)->bool { Self::of(m,self.side,self.index).as_ref()==Some(self) }
}
/// Identity actually painted on a hand-built row, checked before accepting a tap.
#[derive(Component)]
pub(crate) struct RenderedRow(RowStamp);
impl RenderedRow {
    pub(crate) fn of(m:&MenuModel,side:Side,index:usize)->Option<Self> {
        RowStamp::of(m,side,index).map(Self)
    }
}
#[derive(Clone)]
struct Guard { mode:Mode, category:[Category;2], quantity:Option<Quantity>,
    focus:Side, selected:[usize;2], row:Option<RowStamp>, trade:Option<Trade> }
impl Guard {
    fn of(m:&MenuModel,target:Target)->Self {
        let row=match target {
            Target::Row(s,i)=>RowStamp::of(m,s,i),
            _=>m.quantity.and_then(|q|RowStamp::of(m,q.side,q.row)),
        };
        Self { mode:m.mode,category:m.category,quantity:m.quantity,focus:m.focus,selected:m.selected,
            row,trade:m.trade.clone() }
    }
    fn matches(&self,m:&MenuModel)->bool {self.matches_quantity(m,m.quantity)}
    fn matches_quantity(&self,m:&MenuModel,quantity:Option<Quantity>)->bool {
        self.mode==m.mode && self.category==m.category && self.quantity==quantity
            && self.focus==m.focus && self.selected==m.selected
            && self.row.as_ref().is_none_or(|r|r.matches(m))
            && self.trade==m.trade
    }
}
/// Source state attached by the ordinary painter, not reconstructed from the
/// current model at a later touch. A stale frame cannot approve a new quote.
#[derive(Component,Clone)]
pub(crate) struct RenderedMenu {guard:Guard,text_size:f32,status:Option<String>}
impl RenderedMenu {
    pub(crate) fn of(m:&MenuModel,text_size:f32)->Self {Self {guard:Guard::of(m,Target::Command(Command::Accept)),text_size,status:m.status.clone()}}
    pub(super) fn quantity_current(&self,m:&MenuModel)->bool {self.status==m.status && self.guard.matches(m)}
    pub(super) fn amount_repaint(&self,m:&MenuModel,text_size:f32)->bool {
        let (Some(mut current),Some(before))=(m.quantity,self.guard.quantity) else{return false};
        current.amount=before.amount;
        self.text_size==text_size && self.status==m.status && self.guard.matches_quantity(m,Some(current))
    }
}
#[derive(Clone,Copy,PartialEq)]
enum MeterShape { Node([Vec2;3]), Tile([f64;4]) }
#[derive(Clone,Copy)]
struct MeterContact { shape:MeterShape, repaint:bool }
#[derive(Clone,Copy)]
struct MeterRequest { origin:Vec2, shape:MeterShape, pulse:bool }
struct MeterReceipt { token:u64,epoch:u64,window:Entity,id:u64,guard:Guard,amount:i32 }
struct Contact { window:Entity,id:u64,stamp:Stamp,origin:Vec2,previous:Vec2,
    hit:Hit,guard:Guard,dragged:bool,vertical:bool,remainder:f64,meter:Option<MeterContact> }
struct Tap { frame:Frame,id:u64,origin:Vec2,point:Vec2,source:Source,target:Target,swipe:bool,
    guard:Guard,commands:[Option<Command>;2],meter:Option<MeterRequest> }
struct Scroll { frame:Frame,id:u64,mode:Mode,category:[Category;2],side:Side,
    pixels:f64,rows:i64,native:bool }
struct Armed { token:u64,epoch:u64,tap:Tap }
#[derive(Clone,Copy)]
pub(crate) struct NativeTap {
    pub token:u64,pub frame:Frame,pub id:u64,pub point:Vec2,pub source:Source,pub target:Target,pub text_size:f32,
}

#[derive(Resource,Default)]
pub struct NormalBarter {
    frame:Option<Frame>,return_frame:Option<Frame>,stamp:Option<Stamp>,mode:Option<Mode>,epoch:u64,
    contact:Option<Contact>,pending:Option<Tap>,scroll:Option<Scroll>,armed:Option<Armed>,
    native:Option<NativeTap>,
    retired:Vec<(Entity,u64)>,overflow:Option<Entity>,hidden:bool,barrier:bool,draining:Option<Entity>,
    secondary:bool,next:u64,
}
impl NormalBarter {
    fn retire(&mut self,w:Entity,id:u64) {
        if self.retired.contains(&(w,id)){return}
        if self.retired.len()<MAX_IDS {self.retired.push((w,id));}
        else {
            self.overflow=Some(w);self.contact=None;self.pending=None;self.scroll=None;self.armed=None;self.native=None;
            self.draining=Some(w);self.epoch=self.epoch.wrapping_add(1);
        }
    }
    fn meter(&mut self,f:Frame,c:&Contact,point:Vec2,hit:Hit,pulse:bool) {
        let Some(meter)=c.meter else{return};
        let Some(q)=c.guard.quantity else{return};
        let Target::Meter(part)=hit.target else{return};
        let amount=super::model::amount_at(part,q.max);
        // Do not dirty/rebuild the ordinary prompt for unchanged live values.
        // Coalescing back to the current amount also removes an earlier sample.
        if amount==q.amount && !pulse {
            if self.pending.as_ref().is_some_and(|t|t.frame==f && t.id==c.id && t.meter.is_some()){self.pending=None;}
            return
        }
        let command=Command::Amount(amount);
        if let Some(t)=self.pending.as_mut().filter(|t|t.frame==f && t.id==c.id && t.meter.is_some()) {
            t.origin=c.origin;t.point=point;t.source=hit.source;t.target=hit.target;t.swipe=false;t.commands=[Some(command),None];
            t.meter=Some(MeterRequest {origin:c.origin,shape:meter.shape,pulse});
        } else {
            self.pending=Some(Tap {frame:f,id:c.id,origin:c.origin,point,source:hit.source,target:hit.target,swipe:false,
                guard:c.guard.clone(),commands:[Some(command),None],
                meter:Some(MeterRequest {origin:c.origin,shape:meter.shape,pulse})});
        }
    }
    pub fn cancel(&mut self) {
        if let Some(w)=self.contact.as_ref().map(|c|c.window)
            .or_else(||self.pending.as_ref().map(|t|t.frame.window))
            .or_else(||self.armed.as_ref().map(|a|a.tap.frame.window)) {self.draining=Some(w);}
        if let Some(c)=self.contact.take(){self.retire(c.window,c.id);}
        if let Some(t)=self.pending.take(){self.retire(t.frame.window,t.id);}
        if let Some(a)=self.armed.take(){self.retire(a.tap.frame.window,a.tap.id);}
        self.scroll=None;self.native=None;self.barrier=true;self.epoch=self.epoch.wrapping_add(1);
    }
    /// This witness must mean no held touch or release/cancel edge in any window.
    /// It never clears Touches, raw messages or hardware state.
    pub fn observe_neutral(&mut self,neutral:bool) {
        if neutral {self.retired.clear();self.overflow=None;self.draining=None;self.secondary=false;}
    }
    pub fn hardware_intent(&mut self){self.cancel();self.hidden=true;}
    pub fn restore_touch(&mut self){self.cancel();self.hidden=false;}
    pub fn captures_touch(&self,window:Entity,id:u64)->bool {
        self.retired.contains(&(window,id)) || self.overflow==Some(window)
            || self.frame.is_some_and(|f|f.window==window && !self.hidden)
    }
    pub fn visible(&self)->bool {self.frame.is_some() && !self.hidden}
    pub(crate) fn take_native(&mut self)->Option<NativeTap> {
        let n=self.native.take()?;
        self.armed.as_ref().filter(|a|a.token==n.token && a.epoch==self.epoch
            && self.frame==Some(n.frame) && self.stamp==Some(Stamp::of(n.frame,n.text_size))
            && !self.hidden).map(|_|n)
    }
    pub(crate) fn native_commands(&self,n:NativeTap,commands:&[Command])->Option<u64> {
        let a=self.armed.as_ref().filter(|a|a.token==n.token && a.epoch==self.epoch)?;
        a.tap.commands.into_iter().flatten().eq(commands.iter().copied()).then_some(n.token)
    }
    pub(crate) fn finish_native(&mut self,token:u64,emitted:bool) {
        if !emitted && self.armed.as_ref().is_some_and(|a|a.token==token){self.armed=None;}
    }
}

#[derive(SystemParam)]
struct Admission<'w,'s> {
    live:super::barter_touch::LiveAdmission<'w,'s>,
    script:Option<Res<'w,crate::script::ScriptState>>,
    creator:Option<Res<'w,crate::creator::CreatorMenu>>,
}
impl Admission<'_,'_> {
    fn allows(&self,f:Frame,id:u64,m:&MenuModel)->bool {
        (matches!(m.mode,Mode::Barter(_)) || m.quantity.is_some()) && self.live.allows(f,id)
            && super::barter_touch::menu_allows(self.script.as_deref(),self.creator.as_deref(),m)
    }
}
type NodeData=(&'static ComputedNode,&'static UiGlobalTransform,&'static ComputedUiTargetCamera,
    &'static InheritedVisibility,Option<&'static CalculatedClip>,
    Option<&'static RowButton>,Option<&'static CmdButton>,Option<&'static AmountMeter>,
    Option<&'static RenderedRow>,Option<&'static FocusPolicy>,
    Option<&'static super::CategoryTitle>,Option<&'static super::look::QuantityBackdrop>);
#[derive(SystemParam)]
struct Geometry<'w,'s> {
    stack:Option<Res<'w,UiStack>>,
    nodes:Query<'w,'s,NodeData>,
    tiles:Option<Res<'w,crate::tiles::barter_menu::BarterTiles>>,
    parents:Query<'w,'s,&'static ChildOf>,
    menus:Query<'w,'s,&'static RenderedMenu>,
    readability:Option<Res<'w,crate::menu_readability::MenuReadability>>,
}
impl Geometry<'_,'_> {
    fn text_size_for(&self,m:Option<&MenuModel>)->f32 {
        if m.is_some_and(|m|matches!(m.mode,Mode::Barter(_))) {
            super::readability::size(self.readability.as_deref(),crate::menu_readability::MenuKind::Barter)
        } else {1.0}
    }
    fn meter(&self,script:Option<&crate::script::ScriptState>,f:Frame,m:&MenuModel,
        origin:Vec2,point:Vec2)->Option<(Hit,MeterShape)> {
        let mut hit=self.hit(script,f,m,origin)?;
        if !matches!(hit.target,Target::Meter(_)){return None}
        let (part,shape)=match hit.source {
            Source::Node(entity)=>{
                let (node,at,..)=self.nodes.get(entity).ok()?;
                let basis=[node.normalize_point(*at,Vec2::ZERO)?,node.normalize_point(*at,Vec2::X)?,node.normalize_point(*at,Vec2::Y)?];
                if !basis.iter().all(|v|v.is_finite()){return None}
                (node.normalize_point(*at,point*f.density)?.x+0.5,MeterShape::Node(basis))
            }
            Source::Tile(..)|Source::Direct(..)=>{
                let ui=&script?.rt.as_ref()?.game.ui_tree;
                let b=crate::tiles::menus::meter_box(ui)?;
                let bounds=[b.left,b.top,b.width,b.height];
                if !bounds.iter().all(|v|v.is_finite()) || b.width<=0.0 || b.height<=0.0{return None}
                let at=crate::tiles::input::to_menu_units(point,f.size,Vec2::splat(0.5));
                (((at.0-b.left)/b.width) as f32,MeterShape::Tile(bounds))
            }
        };
        if !part.is_finite(){return None}
        hit.target=Target::Meter(part.clamp(0.0,1.0));Some((hit,shape))
    }

    fn hit(&self,script:Option<&crate::script::ScriptState>,f:Frame,m:&MenuModel,point:Vec2)->Option<Hit> {
        let text_size = self.text_size_for(Some(m));
        // Tiled QuantityMenu can appear over a hand-built Barter, Container, or Inventory
        // menu (e.g. Android's default TileMenus includes QuantityMenu but not BarterMenu).
        // Hit-test the tiled QuantityMenu independently of BarterTiles so touch never soft-locks.
        if m.quantity.is_some() {
            if let Some(rt) = script.and_then(|s| s.rt.as_ref()) {
                if crate::tiles::menus::normal_touch_quantity_shown(&rt.game.ui_tree) {
                    return crate::tiles::menus::normal_touch_quantity_hit(&rt.game.ui_tree, point, f.size);
                }
            }
        }
        if let Some(tiles)=self.tiles.as_ref().filter(|t|t.menu().is_some()) {
            if tiles.normal_text_scale()!=text_size{return None}
            let rt=script?.rt.as_ref()?;
            if m.quantity.is_none() || tiles.normal_touch_quantity(&rt.game.ui_tree) {
                return tiles.normal_touch_hit(&rt.game.ui_tree,m,point,f.size);
            }
        }
        let physical=point*f.density;
        for &entity in self.stack.as_ref()?.uinodes.iter().rev() {
            let Ok((node,transform,camera,visible,clip,row,command,meter,painted,focus,cat_title,backdrop))=self.nodes.get(entity) else {continue};
            if camera.get()!=Some(f.camera) || !visible.get() || !node.size().is_finite()
                || node.size().min_element()<=0.0 || !node.contains_point(*transform,physical)
                || clip.is_some_and(|c|!c.contains_point(physical)) {continue}
            let mut ancestor=Some(entity);
            let mut current=false;
            while let Some(parent)=ancestor {
                if let Ok(painted)=self.menus.get(parent) {current=painted.guard.matches(m) && painted.text_size==text_size;break}
                ancestor=self.parents.get(parent).ok().map(ChildOf::parent);
            }
            if !current {continue}
            let target=if let Some(row)=row {
                if m.quantity.is_some() || painted.is_none_or(|p|!p.0.matches(m)){continue}
                Target::Row(row.0,row.1)
            } else if let Some(command)=command {
                if m.quantity.is_some() && !matches!(command.0,Command::Amount(_)|Command::AmountStep(_)|Command::AmountOk|Command::AmountCancel) {
                    continue
                }
                Target::Command(command.0)
            }
            else if meter.is_some() {
                let part=node.normalize_point(*transform,physical)?.x+0.5;
                Target::Meter(part.clamp(0.0,1.0))
            } else if m.quantity.is_none() && cat_title.is_some() {
                Target::CategoryHeader(cat_title.unwrap().0)
            } else if m.quantity.is_some() && backdrop.is_some() {
                Target::Command(Command::AmountCancel)
            } else {
                if focus.is_some_and(|f|*f==FocusPolicy::Block){return None}
                continue
            };
            return Some(Hit {source:Source::Node(entity),target,
                pane:if let Target::Row(s,_)=target {Some(s)} else {None},
                row_step:node.size().y/f.density});
        }
        None
    }
}

/// Parent's sole-reader adapter. There is no MessageReader or virtual mouse here.
#[derive(SystemParam)]
pub struct Bridge<'w,'s> {
    state:ResMut<'w,NormalBarter>,active:Option<Res<'w,ActiveMenu>>,
    options:Option<Res<'w,super::barter_touch::Options>>,
    admission:Admission<'w,'s>,geometry:Geometry<'w,'s>,
}
impl Bridge<'_,'_> {
    fn meter_hit(&self,f:Frame,m:&MenuModel,c:&mut Contact,point:Vec2)->Option<Hit> {
        let meter=c.meter.as_mut()?;
        if !c.guard.matches(m){return None}
        let (hit,shape)=self.geometry.meter(self.admission.script.as_deref(),f,m,c.origin,point)?;
        let same_source=hit.source==c.hit.source
            || (meter.repaint && matches!((hit.source,c.hit.source),(Source::Node(_),Source::Node(_))));
        if shape!=meter.shape || !same_source{return None}
        c.hit=hit;meter.repaint=false;Some(hit)
    }

    /// Once per real parent replay frame, before its raw batch. Unconsumed work
    /// expires here instead of being retried when a later native menu reappears.
    pub fn prepare(&mut self,f:Frame) {
        self.state.pending=None;self.state.scroll=None;self.state.armed=None;self.state.native=None;
        self.state.observe_neutral(f.neutral);self.state.barrier=false;
        let active_m = self.active.as_ref().and_then(|a|a.0.as_ref());
        let mode=active_m.filter(|m|matches!(m.mode,Mode::Barter(_)) || m.quantity.is_some()).map(|m|m.mode);
        let stamp=Stamp::of(f,self.geometry.text_size_for(active_m));
        if self.state.stamp!=Some(stamp) || self.state.mode!=mode || !f.admitted || f.fresh
            || self.options.as_ref().is_some_and(|o|!o.enabled) {
            let had_owner=self.state.contact.is_some() || self.state.pending.is_some() || self.state.armed.is_some();
            self.state.cancel();
            if !had_owner && f.admitted && !f.fresh {self.state.barrier=false;}
        }
        if self.state.stamp.is_some_and(|old|old.window!=f.window){
            self.state.secondary=false;
            if f.admitted && !f.fresh {self.state.barrier=false;}
        }
        self.state.mode=mode;self.state.stamp=Some(stamp);
        self.state.return_frame=mode.filter(|_|f.admitted
            && self.options.as_ref().is_none_or(|o|o.enabled)).map(|_|f);
        self.state.frame=mode.filter(|_|f.admitted && !f.fresh
            && self.options.as_ref().is_none_or(|o|o.enabled)).map(|_|f);
        if let Some(mut c)=self.state.contact.take() {
            let valid=c.meter.is_none() || self.state.frame.is_some()
                && self.active.as_ref().and_then(|a|a.0.as_ref()).is_some_and(|m|{
                    let point=c.previous;self.meter_hit(f,m,&mut c,point).is_some()
                });
            self.state.contact=Some(c);
            if !valid{self.state.cancel();}
        }
    }
    pub fn event(&mut self,e:&TouchInput)->bool {
        if self.state.retired.contains(&(e.window,e.id)) || self.state.overflow==Some(e.window){return true}
        let Some(f)=self.state.frame.filter(|f|f.window==e.window) else{return false};
        let Some(m)=self.active.as_ref().and_then(|a|a.0.as_ref()) else{return false};
        if !self.admission.allows(f,e.id,m) || !e.position.is_finite() {
            self.state.cancel();return true
        }
        if e.phase==TouchPhase::Started && self.state.hidden {
            self.state.restore_touch();self.state.retire(e.window,e.id);self.state.draining=Some(e.window);return true
        }
        if self.state.hidden{return false}
        if self.state.barrier || self.state.draining==Some(e.window) {
            if e.phase==TouchPhase::Started{self.state.retire(e.window,e.id);}
            return true
        }
        if e.phase==TouchPhase::Started {
            if self.state.contact.is_some() {
                if self.state.contact.as_ref().is_some_and(|c|c.window==e.window && c.id==e.id) {self.state.cancel();}
                else {self.state.secondary=true;}
                self.state.retire(e.window,e.id);return true
            }
            if let Some(hit)=self.geometry.hit(self.admission.script.as_deref(),f,m,e.position)
                .filter(|h|matches!(h.source,Source::Tile(..)|Source::Direct(..)) || h.row_step.is_finite() && h.row_step>0.0) {
                let meter=if matches!(hit.target,Target::Meter(_)) {
                    if m.quantity.is_none(){self.state.retire(e.window,e.id);return true}
                    let Some((_,shape))=self.geometry.meter(self.admission.script.as_deref(),f,m,e.position,e.position) else {
                        self.state.retire(e.window,e.id);return true
                    };
                    Some(MeterContact {shape,repaint:false})
                } else {None};
                let c=Contact {window:e.window,id:e.id,stamp:Stamp::of(f,self.geometry.text_size_for(Some(m))),
                    origin:e.position,previous:e.position,hit,guard:Guard::of(m,hit.target),
                    dragged:false,vertical:false,remainder:0.0,meter};
                if c.meter.is_some(){self.state.meter(f,&c,e.position,hit,false);}
                self.state.contact=Some(c);
            } else {self.state.retire(e.window,e.id);}
            return true
        }
        if !self.state.contact.as_ref().is_some_and(|c|c.window==e.window && c.id==e.id){return true}
        let mut c=self.state.contact.take().unwrap();
        if e.phase==TouchPhase::Canceled || c.stamp!=Stamp::of(f,self.geometry.text_size_for(Some(m)))
            || c.guard.mode!=m.mode || c.guard.category!=m.category || c.guard.quantity!=m.quantity {
            self.state.pending=None;self.state.scroll=None;self.state.barrier=true;
            self.state.draining=Some(e.window);
            self.state.retire(e.window,e.id);return true
        }
        c.dragged|=e.position.distance(c.origin)>SLOP;
        if c.meter.is_some() {
            let Some(hit)=self.meter_hit(f,m,&mut c,e.position) else {
                self.state.contact=Some(c);self.state.cancel();return true
            };
            let pulse=e.phase==TouchPhase::Ended && !c.dragged
                && self.geometry.hit(self.admission.script.as_deref(),f,m,e.position)
                    .is_some_and(|end|end.source==hit.source && same_target(end.target,hit.target));
            self.state.meter(f,&c,e.position,hit,pulse);
            c.previous=e.position;
            if e.phase==TouchPhase::Ended {
                self.state.barrier=true;
                if self.state.secondary{self.state.draining=Some(e.window);}
                self.state.retire(e.window,e.id);
            } else {self.state.contact=Some(c);}
            return true
        }
        if c.dragged && c.hit.pane.is_some() {
            let d=e.position-c.origin;c.vertical|=d.y.abs()>=d.x.abs();
        }
        if let Some(side)=c.hit.pane.filter(|_|c.vertical) {
            c.remainder+=f64::from(c.previous.y)-f64::from(e.position.y);
            let native=matches!(c.hit.source,Source::Tile(..));
            let (pixels,rows)=if native {
                let pixels=c.remainder;c.remainder=0.0;(pixels,0)
            } else {
                // Ordinary hand-built lists draw whole rows. Quantize from this
                // contact's actual painted row height (wrapping/scale included),
                // not a fixed logical-pixel constant; no 1:1 pixel-scroll claim.
                let step=f64::from(c.hit.row_step);
                let rows=(c.remainder/step).trunc() as i64;
                c.remainder%=step;
                (0.0,rows)
            };
            if pixels!=0.0 || rows!=0 {
                match self.state.scroll.as_mut() {
                    Some(s) if s.side==side=>{s.pixels+=pixels;s.rows=s.rows.saturating_add(rows);}
                    None=>self.state.scroll=Some(Scroll {frame:f,id:e.id,mode:m.mode,
                        category:m.category,side,pixels,rows,native}),
                    _=>{}
                }
            }
        }
        c.previous=e.position;
        if e.phase==TouchPhase::Ended {
            if let Target::CategoryHeader(side) = c.hit.target {
                let d = e.position - c.origin;
                if c.guard.matches(m) && d.x.abs() >= HEADER_SWIPE && d.x.abs() >= d.y.abs() {
                    let dir = if d.x < 0.0 { -1 } else { 1 };
                    let source = match c.hit.source {
                        Source::Tile(menu, tile) => Source::Direct(menu, tile),
                        other => other,
                    };
                    self.state.pending = Some(Tap {
                        frame: f,
                        id: e.id,
                        origin: c.origin,
                        point: e.position,
                        source,
                        target: Target::CategoryHeader(side),
                        swipe: true,
                        guard: c.guard,
                        commands: [Some(Command::Category(side, dir)), None],
                        meter: None,
                    });
                } else if !c.dragged && c.guard.matches(m) {
                    if let Some(hit) = self.geometry.hit(self.admission.script.as_deref(), f, m, e.position)
                        .filter(|h| h.source == c.hit.source && same_target(h.target, c.hit.target))
                    {
                        let source = match hit.source {
                            Source::Tile(menu, tile) => Source::Direct(menu, tile),
                            other => other,
                        };
                        self.state.pending = Some(Tap {
                            frame: f,
                            id: e.id,
                            origin: c.origin,
                            point: e.position,
                            source,
                            target: hit.target,
                            swipe: false,
                            guard: c.guard,
                            commands: [Some(Command::Category(side, 1)), None],
                            meter: None,
                        });
                    }
                }
            } else if !c.dragged && c.guard.matches(m) {
                if let Some(hit)=self.geometry.hit(self.admission.script.as_deref(),f,m,e.position)
                    .filter(|h|h.source==c.hit.source && same_target(h.target,c.hit.target)) {
                    self.state.pending=Some(Tap {frame:f,id:e.id,origin:c.origin,point:e.position,
                        source:hit.source,target:hit.target,swipe:false,guard:c.guard,
                        commands:commands(hit.target,m),meter:None});
                }
            }
            self.state.barrier=true;
            if self.state.secondary{self.state.draining=Some(e.window);}
            self.state.retire(e.window,e.id);
        } else {self.state.contact=Some(c);}
        true
    }
    pub fn captures_touch(&self,w:Entity,id:u64)->bool {self.state.captures_touch(w,id)}
    pub fn visible(&self)->bool {self.state.visible()}
    pub fn committed_this_batch(&self)->bool {self.state.barrier && self.state.pending.is_some()}
    pub fn cancel(&mut self){self.state.cancel();self.state.frame=None;self.state.return_frame=None;}
    pub fn hardware_intent(&mut self){self.state.hardware_intent();}
    pub fn restore_touch(&mut self){self.state.restore_touch();}
    pub fn observe_neutral(&mut self,n:bool){self.state.observe_neutral(n);}
    /// The parent may call this only with the actual selector-consumed Started
    /// edge, after preparing this current normal menu and actual primary frame.
    pub fn consume_touch_return(&mut self,e:&TouchInput)->bool {
        let Some(f)=self.state.return_frame.filter(|f|f.window==e.window) else{return false};
        let Some(m)=self.active.as_ref().and_then(|a|a.0.as_ref()) else{return false};
        if e.phase!=TouchPhase::Started || (!matches!(m.mode,Mode::Barter(_)) && m.quantity.is_none())
            || !self.admission.live.allows_consumed_return(f,e.id)
            || !super::barter_touch::menu_allows(self.admission.script.as_deref(),
                self.admission.creator.as_deref(),m) {return false}
        self.state.restore_touch();self.state.retire(e.window,e.id);self.state.draining=Some(e.window);true
    }
}
pub(crate) fn same_target(a:Target,b:Target)->bool {
    match(a,b){(Target::Meter(_),Target::Meter(_))=>true,_=>a==b}
}
fn commands(t:Target,m:&MenuModel)->[Option<Command>;2] {
    match t {
        Target::Row(s,i)=>{
            let v=crate::tiles::container_menu::row_commands(s,i,m.focus==s && m.selected[s as usize]==i,false);
            [v.first().copied(),v.get(1).copied()]
        }
        Target::Command(c)=>[Some(c),None],
        Target::CategoryHeader(side)=>[Some(Command::Category(side,1)),None],
        Target::Meter(part)=>[m.quantity.map(|q|Command::Amount(super::model::amount_at(part,q.max))),None],
        Target::Pane(_)=>[None,None],
    }
}

fn dispatch(mut state:ResMut<NormalBarter>,mut active:ResMut<ActiveMenu>,
    live:super::barter_touch::LiveAdmission,
    (mut script,creator):(Option<ResMut<crate::script::ScriptState>>,Option<Res<crate::creator::CreatorMenu>>),
    mut tiles:Option<ResMut<crate::tiles::barter_menu::BarterTiles>>,mut out:MessageWriter<MenuInput>,
    readability:Option<Res<crate::menu_readability::MenuReadability>>) {
    if state.scroll.is_none() && state.pending.is_none(){return}
    let Some(m)=active.0.as_mut() else{state.cancel();return};
    let text_size=if matches!(m.mode,Mode::Barter(_)) {
        super::readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Barter)
    } else {1.0};
    if let Some(s)=state.scroll.take() {
        if state.frame==Some(s.frame) && state.stamp==Some(Stamp::of(s.frame,text_size))
            && s.mode==m.mode && s.category==m.category && m.quantity.is_none()
            && live.allows(s.frame,s.id)
            && super::barter_touch::menu_allows(script.as_deref(),creator.as_deref(),m) {
            if s.native {
                if let Some((tiles,rt))=tiles.as_mut().zip(script.as_mut().and_then(|s|s.rt.as_mut())) {
                    tiles.normal_touch_scroll(&mut rt.game.ui_tree,s.side,s.pixels,s.frame.size);
                }
            } else {
                let i=s.side as usize;
                m.scroll[i]=(m.scroll[i] as i64).saturating_add(s.rows)
                    .clamp(0,m.list(s.side).rows.len().saturating_sub(1) as i64) as usize;
            }
            m.pointer_pick=true;
        }
    }
    let Some(tap)=state.pending.take() else{return};
    if state.frame!=Some(tap.frame) || state.stamp!=Some(Stamp::of(tap.frame,text_size)) || !live.allows(tap.frame,tap.id)
        || !super::barter_touch::menu_allows(script.as_deref(),creator.as_deref(),m) {return}
    if matches!(tap.source,Source::Tile(..)|Source::Direct(..)) && script.as_ref().is_none_or(|s|s.rt.is_none()){return}
    state.next=state.next.wrapping_add(1).max(1);
    let token=state.next;let epoch=state.epoch;
    let native=(matches!(tap.source,Source::Tile(..)) && !tap.swipe && tap.meter.is_none_or(|m|m.pulse)).then_some(NativeTap {
        token,frame:tap.frame,id:tap.id,point:tap.point,source:tap.source,target:tap.target,text_size });
    state.armed=Some(Armed {token,epoch,tap});
    if let Some(native)=native {state.native=Some(native);}
    else {out.write(MenuInput(Command::CommitNormalBarter(token)));}
}

/// Native feed validates the same current gates, lending its already-owned
/// ScriptState. It must not add an aliased ScriptState SystemParam.
#[derive(SystemParam)]
pub(crate) struct NativeAdmission<'w,'s> {
    readability:Option<Res<'w,crate::menu_readability::MenuReadability>>,
    live:super::barter_touch::LiveAdmission<'w,'s>,
    options:Option<Res<'w,super::barter_touch::Options>>,
    creator:Option<Res<'w,crate::creator::CreatorMenu>>,
}
impl NativeAdmission<'_,'_> {
    pub(crate) fn allows(&self,n:NativeTap,m:&MenuModel,game:&fnv_game::Game<'_>)->bool {
        let expected_size = if matches!(m.mode,Mode::Barter(_)) {
            super::readability::size(self.readability.as_deref(),crate::menu_readability::MenuKind::Barter)
        } else {1.0};
        (matches!(m.mode,Mode::Barter(_)) || m.quantity.is_some()) && self.options.as_ref().is_none_or(|o|o.enabled)
            && n.text_size==expected_size
            && self.live.allows(n.frame,n.id)
            && super::barter_touch::game_allows(Some(game),self.creator.as_deref(),m)
    }
}
#[derive(SystemParam)]
pub(crate) struct Commits<'w,'s> {
    state:Option<ResMut<'w,NormalBarter>>,
    options:Option<Res<'w,super::barter_touch::Options>>,
    live:super::barter_touch::LiveAdmission<'w,'s>,
    creator:Option<Res<'w,crate::creator::CreatorMenu>>,
    geometry:Geometry<'w,'s>,
    applied:Local<'s,Option<MeterReceipt>>,
}
impl Commits<'_,'_> {
    pub(crate) fn discard(&mut self,token:u64){
        if let Some(s)=self.state.as_mut() {
            if s.armed.as_ref().is_some_and(|a|a.token==token){s.armed=None;}
        }
    }
    /// Acknowledge only the original consumer's completed Amount. Foreign prompt
    /// changes retire the owner, including cancel/reopen within one queue batch.
    pub(crate) fn applied(&mut self,token:Option<u64>,command:Command,before:Option<Quantity>,m:&MenuModel) {
        let Some(s)=self.state.as_mut() else{return};
        if self.applied.as_ref().is_some_and(|r|Some(r.token)==token) {
            let mut r=self.applied.take().unwrap();
            if let Some(q)=r.guard.quantity.as_mut(){q.amount=r.amount;}
            if command!=Command::Amount(r.amount) || r.epoch!=s.epoch || !r.guard.matches(m) {
                s.cancel();return
            }
            if let Some(c)=s.contact.as_mut().filter(|c|c.window==r.window && c.id==r.id) {
                if let Some(meter)=c.meter.as_mut() {
                    // A successfully consumed Amount dirties ActiveMenu even if clamped/no-op.
                    meter.repaint=true;c.guard=r.guard;
                }
            }
        } else if before!=m.quantity && s.contact.as_ref().is_some_and(|c|c.meter.is_some()) {
            s.cancel();
        }
    }
    pub(crate) fn resolve(&mut self,token:u64,m:&mut MenuModel,world:&mut fnv_sim::World<'_>,
        script:Option<&crate::script::ScriptState>)->Option<[Option<Command>;2]> {
        let s=self.state.as_mut()?;
        if s.armed.as_ref()?.token!=token{return None}
        let a=s.armed.take()?;*self.applied=None;
        let live=a.epoch==s.epoch && !s.hidden && s.frame==Some(a.tap.frame)
            && s.stamp==Some(Stamp::of(a.tap.frame,self.geometry.text_size_for(Some(m))))
            && self.options.as_ref().is_none_or(|o|o.enabled)
            && self.live.allows(a.tap.frame,a.tap.id)
            && super::barter_touch::menu_allows(script,self.creator.as_deref(),m);
        if !live {if a.tap.meter.is_some(){s.cancel();}return None}
        let refreshed=m.refresh(world).is_ok() && a.tap.guard.matches(m);
        let source_matches = |actual: Source, expected: Source| {
            actual == expected
                || matches!((actual, expected), (Source::Tile(m1, t1), Source::Direct(m2, t2)) if m1 == m2 && t1 == t2)
        };
        let geometry=refreshed && if let Some(request)=a.tap.meter {
            self.geometry.meter(script,a.tap.frame,m,request.origin,a.tap.point)
                .is_some_and(|(h,shape)|source_matches(h.source,a.tap.source) && shape==request.shape
                    && same_target(h.target,a.tap.target) && commands(h.target,m)==a.tap.commands)
        } else if a.tap.swipe {
            self.geometry.hit(script,a.tap.frame,m,a.tap.origin)
                .is_some_and(|h|source_matches(h.source,a.tap.source) && same_target(h.target,a.tap.target))
        } else {
            self.geometry.hit(script,a.tap.frame,m,a.tap.point)
                .is_some_and(|h|source_matches(h.source,a.tap.source) && same_target(h.target,a.tap.target))
        };
        if !geometry {
            if a.tap.meter.is_some(){s.cancel();}
            m.status=Some("Trade changed. Review the item and amount again.".into());return None
        }
        if a.tap.meter.is_some() && s.contact.as_ref().is_some_and(|c|c.window==a.tap.frame.window && c.id==a.tap.id) {
            if let Some(Command::Amount(amount))=a.tap.commands[0] {
                *self.applied=Some(MeterReceipt {token,epoch:s.epoch,window:a.tap.frame.window,id:a.tap.id,
                    guard:a.tap.guard,amount});
            }
        }
        Some(a.tap.commands)
    }
}
/// Read the real normal-menu hit consumer in its owning crate's fixture context.
/// This installs no input reader and changes no pointer, amount or ownership state.
#[cfg(test)]
pub(super) fn meter_fixture_hit(world:&mut World,frame:Frame,point:Vec2)->Option<Hit> {
    use bevy::ecs::system::RunSystemOnce;
    world.run_system_once(move |geometry:Geometry,active:Res<ActiveMenu>,
        script:Option<Res<crate::script::ScriptState>>| {
        active.0.as_ref().and_then(|m|geometry.hit(script.as_deref(),frame,m,point))
    }).expect("the actual meter hit consumer has its real fixture providers")
}

#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct Dispatch;
pub(super) fn install(app:&mut App){
    app.init_resource::<NormalBarter>().add_systems(Update,
        dispatch.in_set(Dispatch).before(super::MenuSystems).before(crate::tiles::input::feed)
            .run_if(resource_exists::<ActiveMenu>));
}
