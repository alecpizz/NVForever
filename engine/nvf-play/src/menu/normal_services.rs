//! Finger input over the existing repair-services and workbench presentations.
//! The parent owns the sole raw reader. These envelopes are resolved by the
//! original keys consumers; they do not implement repair or crafting rules.
use std::sync::Arc;
use super::{barter_touch::{Frame,LiveAdmission},normal_touch::Source};
use bevy::{ecs::system::SystemParam,input::touch::{TouchInput,TouchPhase},prelude::*,
    ui::{CalculatedClip,ComputedUiTargetCamera,FocusPolicy,UiGlobalTransform,UiStack}};

const SLOP:f32=12.0;
const HEADER_SWIPE:f32=28.0;
const MAX_IDS:usize=32;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind {Vendor,Workbench}

/// Source state from the actual painter/consumer. Geometry's first row and
/// cosmetic output effects are compared separately from transactional identity.
#[derive(Clone,Debug)]
pub(crate) enum Guard {
    Vendor {vendor:fnv_sim::FormId,view:crate::repair::View},
    Workbench {category:fnv_sim::FormId,station:Option<fnv_sim::FormId>,filter:usize,
        view:Box<crate::crafting::View>},
}
impl Guard {
    pub(crate) fn kind(&self)->Kind {match self {Self::Vendor {..}=>Kind::Vendor,Self::Workbench {..}=>Kind::Workbench}}
    pub(crate) fn same(&self,other:&Self)->bool {
        match(self,other) {
            (Self::Vendor {vendor:a,view:x},Self::Vendor {vendor:b,view:y})=>a==b && x.normal_matches(y),
            (Self::Workbench {category:a,station:sa,filter:fa,view:x},
                Self::Workbench {category:b,station:sb,filter:fb,view:y})=>a==b && sa==sb && fa==fb && x.normal_matches(y),
            _=>false,
        }
    }
    fn same_session(&self,other:&Self)->bool {
        match(self,other) {
            (Self::Vendor {vendor:a,view:x},Self::Vendor {vendor:b,view:y})=>a==b && x.selected==y.selected,
            (Self::Workbench {category:a,station:sa,filter:fa,view:x},
                Self::Workbench {category:b,station:sb,filter:fb,view:y})=>a==b && sa==sb && fa==fb
                    && x.selected==y.selected && x.normal_quantity()==y.normal_quantity(),
            _=>false,
        }
    }
}

#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) enum Action {
    Vendor(crate::repair::RepairServicesInput),
    Workbench(crate::crafting::RecipeInput),
    Amount(super::model::Command),
    Row(usize),
    Pane(usize),
    Browse(usize,i64),
    CategoryHeader,
}
#[derive(Clone,Copy,Debug)]
pub(crate) struct Hit {pub source:Source,pub action:Action,pub pane:Option<usize>,pub row_step:f32}

#[derive(Component)]
pub(crate) struct Rendered {guard:Guard,text_size:f32}
impl Rendered {
    pub(crate) fn new(guard:Guard,text_size:f32)->Self {Self {guard,text_size}}
}
#[derive(Clone,Copy,PartialEq)]
struct Stamp {frame:Frame,units:f32,text_size:f32}
impl Stamp {fn new(frame:Frame,text_size:f32)->Self {Self {frame,units:crate::interface::unit_scale(),text_size}}}
struct Contact {window:Entity,id:u64,origin:Vec2,previous:Vec2,stamp:Stamp,hit:Hit,
    guard:Arc<Guard>,dragged:bool,vertical:bool,remainder:f64}
struct Tap {id:u64,stamp:Stamp,origin:Vec2,point:Vec2,hit:Hit,swipe:bool,guard:Arc<Guard>}
struct Scroll {id:u64,stamp:Stamp,pane:usize,pixels:f64,rows:i64,guard:Arc<Guard>,native:bool,
    source:Source,point:Vec2}
struct Armed {token:u64,epoch:u64,tap:Tap}

#[derive(Clone,Copy)]
pub(crate) struct NativeTap {pub token:u64,pub id:u64,pub frame:Frame,pub source:Source,
    pub point:Vec2,pub action:Action,pub kind:Kind,pub text_size:f32}

#[derive(Resource,Default)]
pub struct NormalServices {
    frame:Option<Frame>,return_frame:Option<Frame>,stamp:Option<Stamp>,kind:Option<Kind>,
    contact:Option<Contact>,pending:Option<Tap>,scroll:Option<Scroll>,armed:Option<Armed>,native:Option<NativeTap>,
    epoch:u64,next:u64,hidden:bool,barrier:bool,draining:Option<Entity>,secondary:bool,
    retired:Vec<(Entity,u64)>,overflow:Option<Entity>,
}
impl NormalServices {
    pub(crate) fn cancel_size(&mut self,kind:crate::menu_readability::MenuKind) {
        let relevant=matches!((self.kind,kind),
            (Some(Kind::Vendor),crate::menu_readability::MenuKind::Repair)
                |(Some(Kind::Workbench),crate::menu_readability::MenuKind::Workbench));
        if relevant {self.cancel();}
    }
    fn retire(&mut self,w:Entity,id:u64) {
        if self.retired.contains(&(w,id)){return}
        if self.retired.len()<MAX_IDS {self.retired.push((w,id));}
        else {self.overflow=Some(w);self.contact=None;self.pending=None;self.scroll=None;
            self.armed=None;self.native=None;self.draining=Some(w);self.epoch=self.epoch.wrapping_add(1);}
    }
    pub fn cancel(&mut self) {
        if let Some(w)=self.contact.as_ref().map(|c|c.window)
            .or_else(||self.pending.as_ref().map(|t|t.stamp.frame.window))
            .or_else(||self.armed.as_ref().map(|a|a.tap.stamp.frame.window)){self.draining=Some(w);}
        if let Some(c)=self.contact.take(){self.retire(c.window,c.id);}
        if let Some(t)=self.pending.take(){self.retire(t.stamp.frame.window,t.id);}
        if let Some(a)=self.armed.take(){self.retire(a.tap.stamp.frame.window,a.tap.id);}
        self.scroll=None;self.native=None;self.barrier=true;self.epoch=self.epoch.wrapping_add(1);
    }
    pub fn observe_neutral(&mut self,neutral:bool) {
        if neutral {self.retired.clear();self.overflow=None;self.draining=None;self.secondary=false;}
    }
    pub fn hardware_intent(&mut self){self.cancel();self.hidden=true;}
    pub fn restore_touch(&mut self){self.cancel();self.hidden=false;}
    pub fn visible(&self)->bool {self.frame.is_some() && !self.hidden}
    pub fn captures_touch(&self,window:Entity,id:u64)->bool {
        self.retired.contains(&(window,id)) || self.overflow==Some(window)
            || self.frame.is_some_and(|f|f.window==window && !self.hidden)
    }
    pub(crate) fn take_native(&mut self)->Option<NativeTap> {
        let n=self.native.take()?;
        self.armed.as_ref().filter(|a|a.token==n.token && a.epoch==self.epoch
            && self.stamp==Some(Stamp::new(n.frame,n.text_size)) && !self.hidden).map(|_|n)
    }
    pub(crate) fn native_guard(&self,token:u64)->Option<&Guard> {
        self.armed.as_ref().filter(|a|a.token==token && a.epoch==self.epoch).map(|a|a.tap.guard.as_ref())
    }
    pub(crate) fn native_command(&self,n:NativeTap,action:Action)->Option<u64> {
        self.armed.as_ref().filter(|a|a.token==n.token && a.epoch==self.epoch
            && expected(&a.tap.guard,a.tap.hit.action)==action).map(|_|n.token)
    }
    pub(crate) fn finish_native(&mut self,token:u64,emitted:bool) {
        if !emitted && self.armed.as_ref().is_some_and(|a|a.token==token){self.armed=None;}
    }
}

fn expected(guard:&Guard,action:Action)->Action {
    match(guard,action) {
        (Guard::Vendor {view,..},Action::Row(i))=>Action::Vendor(crate::tiles::repair_services_menu::row_input(Some(view.selected),i)),
        (Guard::Workbench {view,..},Action::Row(i))=>Action::Workbench(crate::tiles::recipe_menu::row_input(Some(view.selected),i)),
        (Guard::Workbench {..},Action::CategoryHeader)=>Action::Workbench(crate::crafting::RecipeInput::Filter(1)),
        (_,action)=>action,
    }
}

fn menu_allows(kind:Kind,game:Option<&fnv_game::Game<'_>>)->bool {
    game.is_none_or(|g| {
        !g.loading && !matches!(g.side_menu(),Some(fnv_game::game::menus::LOADING|fnv_game::game::menus::VATS))
            && g.top_menu().is_none_or(|code|code==match kind {
                Kind::Vendor=>fnv_game::repair::REPAIR_SERVICES_MENU,
                Kind::Workbench=>fnv_game::crafting::RECIPE_MENU,
            })
    })
}

#[derive(SystemParam)]
struct Sources<'w> {
    vendor:Option<Res<'w,crate::repair::RepairServices>>,
    recipe:Option<Res<'w,crate::crafting::RecipeMenu>>,
}
impl Sources<'_> {
    fn capture(&self,kind:Kind)->Option<Guard> {match kind {
        Kind::Vendor=>self.vendor.as_ref()?.normal_guard(),
        Kind::Workbench=>self.recipe.as_ref()?.normal_guard(),
    }}
    fn same_session(&self,guard:&Guard)->bool {match guard {
        Guard::Vendor {vendor,view}=>self.vendor.as_ref().is_some_and(|s|s.normal_same_session(*vendor,view.selected)),
        Guard::Workbench {category,station,filter,view}=>self.recipe.as_ref().is_some_and(|s|
            s.normal_same_session(*category,*station,*filter,view.selected,view.normal_quantity())),
    }}
    fn matches(&self,guard:&Guard)->bool {match guard {
        Guard::Vendor {..}=>self.vendor.as_ref().is_some_and(|s|s.normal_matches(guard)),
        Guard::Workbench {..}=>self.recipe.as_ref().is_some_and(|s|s.normal_matches(guard)),
    }}
}

type NodeData=(&'static ComputedNode,&'static UiGlobalTransform,&'static ComputedUiTargetCamera,
    &'static InheritedVisibility,Option<&'static CalculatedClip>,Option<&'static FocusPolicy>,
    Option<&'static crate::repair::RepairButton>,Option<&'static crate::crafting::RecipeButton>,
    Option<&'static super::CmdButton>,Option<&'static super::AmountMeter>);
#[derive(SystemParam)]
struct Geometry<'w,'s> {
    stack:Option<Res<'w,UiStack>>,nodes:Query<'w,'s,NodeData>,parents:Query<'w,'s,&'static ChildOf>,
    painted:Query<'w,'s,&'static Rendered>,
    vendor:Option<Res<'w,crate::tiles::repair_services_menu::RepairServicesTiles>>,
    recipe:Option<Res<'w,crate::tiles::recipe_menu::RecipeTiles>>,
    readability:Option<Res<'w,crate::menu_readability::MenuReadability>>,
}
impl Geometry<'_,'_> {
    fn size(&self,kind:Kind)->f32 {super::readability::size(self.readability.as_deref(),match kind {
        Kind::Vendor=>crate::menu_readability::MenuKind::Repair,Kind::Workbench=>crate::menu_readability::MenuKind::Workbench})}
    fn hit(&self,game:Option<&fnv_game::Game<'_>>,frame:Frame,guard:&Guard,point:Vec2)->Option<Hit> {
        if !point.is_finite() || !Rect::from_corners(Vec2::ZERO,frame.size).contains(point){return None}
        let kind=guard.kind();let size=self.size(kind);
        if let Some(game)=game {
            match kind {
                Kind::Vendor if self.vendor.as_ref().is_some_and(|t|t.menu.is_some())=>
                    return self.vendor.as_ref()?.normal_hit(&game.ui_tree,frame,guard,point,size),
                Kind::Workbench if self.recipe.as_ref().is_some_and(|t|t.menu.is_some())
                    && !matches!(guard,Guard::Workbench {view,..} if view.normal_quantity().is_some())=>
                    return self.recipe.as_ref()?.normal_hit(&game.ui_tree,frame,guard,point,size),
                _=>{},
            }
        }
        let physical=point*frame.density;
        for &e in self.stack.as_ref()?.uinodes.iter().rev() {
            let Ok((node,t,camera,visible,clip,focus,vendor,recipe,command,meter))=self.nodes.get(e) else{continue};
            if camera.get()!=Some(frame.camera) || !visible.get() || !node.size().is_finite()
                || node.size().min_element()<=0.0 || !node.contains_point(*t,physical)
                || clip.is_some_and(|c|!c.contains_point(physical)){continue}
            let mut ancestor=Some(e);let mut current=false;
            while let Some(parent)=ancestor {
                if let Ok(p)=self.painted.get(parent){current=p.text_size==size && p.guard.same(guard);break}
                ancestor=self.parents.get(parent).ok().map(ChildOf::parent);
            }
            if !current{continue}
            let action=match kind {
                Kind::Vendor=>vendor.map(|button|match button {
                    crate::repair::RepairButton::Row(i)=>Action::Row(*i),
                    crate::repair::RepairButton::All=>Action::Vendor(crate::repair::RepairServicesInput::All),
                    crate::repair::RepairButton::Done=>Action::Vendor(crate::repair::RepairServicesInput::Done),
                }),
                Kind::Workbench=>{
                    if let Some(q)=command {Some(Action::Amount(q.0))}
                    else if meter.is_some(){let part=node.normalize_point(*t,physical)?.x+0.5;
                        let Guard::Workbench {view,..}=guard else{return None};
                        Some(Action::Amount(super::model::Command::Amount(super::model::amount_at(part,view.normal_quantity()?.1))))}
                    else {recipe.map(|button|match button {
                        crate::crafting::RecipeButton::Row(i)=>Action::Row(*i),
                        crate::crafting::RecipeButton::Accept=>Action::Workbench(crate::crafting::RecipeInput::Accept),
                        crate::crafting::RecipeButton::Exit=>Action::Workbench(crate::crafting::RecipeInput::Exit),
                        crate::crafting::RecipeButton::Left=>Action::Workbench(crate::crafting::RecipeInput::Filter(-1)),
                        crate::crafting::RecipeButton::Right=>Action::Workbench(crate::crafting::RecipeInput::Filter(1)),
                        crate::crafting::RecipeButton::Title=>Action::CategoryHeader,
                    })}
                },
            };
            if let Some(action)=action {
                return Some(Hit {source:Source::Node(e),action,
                    pane:if matches!(action,Action::Row(_)){Some(0)}else{None},row_step:node.size().y/frame.density});
            }
            if focus==Some(&FocusPolicy::Block){return None}
        }
        None
    }
}

#[derive(SystemParam)]
pub struct Bridge<'w,'s> {
    state:ResMut<'w,NormalServices>,sources:Sources<'w>,live:LiveAdmission<'w,'s>,
    script:Option<Res<'w,crate::script::ScriptState>>,geometry:Geometry<'w,'s>,
}
impl Bridge<'_,'_> {
    /// Once per parent replay, with its selected current owner. The maintenance
    /// preference is lent through admitted; borrowing it here would alias the
    /// parent's existing maintenance Bridge.
    pub fn prepare(&mut self,kind:Option<Kind>,frame:Frame) {
        self.state.pending=None;self.state.scroll=None;self.state.armed=None;self.state.native=None;
        self.state.observe_neutral(frame.neutral);self.state.barrier=false;
        let kind=kind.filter(|k|match k {
            Kind::Vendor=>self.sources.vendor.as_ref().is_some_and(|s|s.is_open()),
            Kind::Workbench=>self.sources.recipe.as_ref().is_some_and(|s|s.is_open()),
        });
        let size=kind.map_or(1.0,|k|self.geometry.size(k));let stamp=Stamp::new(frame,size);
        if self.state.stamp!=Some(stamp) || self.state.kind!=kind || !frame.admitted || frame.fresh {
            let had=self.state.contact.is_some();self.state.cancel();
            if !had && frame.admitted && !frame.fresh {self.state.barrier=false;}
        }
        if self.state.stamp.is_some_and(|old|old.frame.window!=frame.window) {
            self.state.secondary=false;
            if frame.admitted && !frame.fresh {self.state.barrier=false;}
        }
        self.state.kind=kind;self.state.stamp=Some(stamp);
        self.state.return_frame=kind.filter(|_|frame.admitted).map(|_|frame);
        self.state.frame=kind.filter(|_|frame.admitted && !frame.fresh).map(|_|frame);
    }
    pub fn event(&mut self,e:&TouchInput)->bool {
        if self.state.retired.contains(&(e.window,e.id)) || self.state.overflow==Some(e.window){return true}
        let Some(frame)=self.state.frame.filter(|f|f.window==e.window) else{return false};
        let Some(kind)=self.state.kind else{return false};
        let game=self.script.as_ref().and_then(|s|s.rt.as_ref()).map(|r|&r.game);
        if !e.position.is_finite() || !self.live.allows(frame,e.id) || self.live.creator_open() || !menu_allows(kind,game) {
            self.state.cancel();return true
        }
        if e.phase==TouchPhase::Started && self.state.hidden {
            self.state.restore_touch();self.state.retire(e.window,e.id);self.state.draining=Some(e.window);return true
        }
        if self.state.hidden{return false}
        if self.state.barrier || self.state.draining==Some(e.window) {
            if e.phase==TouchPhase::Started {self.state.retire(e.window,e.id);}
            return true
        }
        if e.phase==TouchPhase::Started {
            if self.state.contact.is_some() {
                if self.state.contact.as_ref().is_some_and(|c|c.window==e.window && c.id==e.id){self.state.cancel();}
                else {self.state.secondary=true;}
                self.state.retire(e.window,e.id);return true
            }
            let Some(guard)=self.sources.capture(kind) else{self.state.retire(e.window,e.id);return true};
            if let Some(hit)=self.geometry.hit(game,frame,&guard,e.position) {
                self.state.contact=Some(Contact {window:e.window,id:e.id,origin:e.position,previous:e.position,
                    stamp:Stamp::new(frame,self.geometry.size(kind)),hit,guard:Arc::new(guard),dragged:false,vertical:false,remainder:0.0});
            } else {self.state.retire(e.window,e.id);}
            return true
        }
        if !self.state.contact.as_ref().is_some_and(|c|c.window==e.window && c.id==e.id){return true}
        let mut contact=self.state.contact.take().unwrap();
        if e.phase==TouchPhase::Canceled || contact.stamp!=Stamp::new(frame,self.geometry.size(kind))
            || !self.sources.same_session(&contact.guard) {
            self.state.cancel();self.state.draining=Some(e.window);self.state.retire(e.window,e.id);return true
        }
        contact.dragged|=e.position.distance(contact.origin)>SLOP;
        if contact.dragged && contact.hit.pane.is_some() {
            let delta=e.position-contact.origin;contact.vertical|=delta.y.abs()>=delta.x.abs();
        }
        if let Some(pane)=contact.hit.pane.filter(|_|contact.vertical) {
            contact.remainder+=f64::from(contact.previous.y)-f64::from(e.position.y);
            let native=matches!(contact.hit.source,Source::Tile(..));
            let (pixels,rows)=if native {let v=contact.remainder;contact.remainder=0.0;(v,0)}
                else if contact.hit.row_step.is_finite() && contact.hit.row_step>0.0 {
                    let step=f64::from(contact.hit.row_step);let rows=(contact.remainder/step).trunc() as i64;
                    contact.remainder%=step;(0.0,rows)
                } else {(0.0,0)};
            if pixels!=0.0 || rows!=0 {
                if let Some(old)=self.state.scroll.as_mut().filter(|s|s.pane==pane) {
                    old.pixels+=pixels;old.rows=old.rows.saturating_add(rows);
                } else if self.state.scroll.is_none() {
                    self.state.scroll=Some(Scroll {id:e.id,stamp:contact.stamp,pane,pixels,rows,
                        guard:contact.guard.clone(),native,source:contact.hit.source,point:e.position});
                }
            }
        }
        contact.previous=e.position;
        if e.phase==TouchPhase::Ended {
            if contact.hit.action == Action::CategoryHeader {
                let d = e.position - contact.origin;
                if self.sources.matches(&contact.guard) && d.x.abs() >= HEADER_SWIPE && d.x.abs() >= d.y.abs() {
                    let dir = if d.x < 0.0 { -1 } else { 1 };
                    let source = match contact.hit.source {
                        Source::Tile(menu, tile) => Source::Direct(menu, tile),
                        other => other,
                    };
                    self.state.pending = Some(Tap {
                        id: e.id,
                        stamp: contact.stamp,
                        origin: contact.origin,
                        point: e.position,
                        hit: Hit {
                            source,
                            action: Action::Workbench(crate::crafting::RecipeInput::Filter(dir)),
                            pane: None,
                            row_step: contact.hit.row_step,
                        },
                        swipe: true,
                        guard: contact.guard,
                    });
                } else if !contact.dragged && self.sources.matches(&contact.guard) {
                    if let Some(hit) = self.geometry.hit(game, frame, &contact.guard, e.position)
                        .filter(|h| h.source == contact.hit.source && same_action(h.action, contact.hit.action))
                    {
                        self.state.pending = Some(Tap {
                            id: e.id,
                            stamp: contact.stamp,
                            origin: contact.origin,
                            point: e.position,
                            hit,
                            swipe: false,
                            guard: contact.guard,
                        });
                    }
                }
            } else if !contact.dragged && self.sources.matches(&contact.guard) {
                if let Some(hit)=self.geometry.hit(game,frame,&contact.guard,e.position)
                    .filter(|h|h.source==contact.hit.source && same_action(h.action,contact.hit.action)) {
                    self.state.pending=Some(Tap {id:e.id,stamp:contact.stamp,origin:contact.origin,point:e.position,hit,swipe:false,guard:contact.guard});
                }
            }
            self.state.barrier=true;if self.state.secondary {self.state.draining=Some(e.window);}
            self.state.retire(e.window,e.id);
        } else {self.state.contact=Some(contact);}
        true
    }
    pub fn cancel(&mut self){self.state.cancel();self.state.frame=None;self.state.return_frame=None;}
    pub fn observe_neutral(&mut self,n:bool){self.state.observe_neutral(n);}
    pub fn captures_touch(&self,w:Entity,id:u64)->bool {self.state.captures_touch(w,id)}
    pub fn visible(&self)->bool {self.state.visible()}
    pub fn hardware_intent(&mut self){self.state.hardware_intent();}
    pub fn restore_touch(&mut self){self.state.restore_touch();}
    pub fn consume_touch_return(&mut self,e:&TouchInput)->bool {
        let Some(f)=self.state.return_frame.filter(|f|f.window==e.window) else{return false};
        let Some(kind)=self.state.kind else{return false};
        let game=self.script.as_ref().and_then(|s|s.rt.as_ref()).map(|r|&r.game);
        if e.phase!=TouchPhase::Started || !self.live.allows_consumed_return(f,e.id) || self.live.creator_open() || !menu_allows(kind,game){return false}
        self.state.restore_touch();self.state.retire(e.window,e.id);self.state.draining=Some(e.window);true
    }
}

/// Typed request resolved in the original owner, after physical selection and
/// against that owner's freshly calculated current view, not a second ruleset.
#[derive(SystemParam)]
pub(crate) struct Commits<'w,'s> {
    state:Option<ResMut<'w,NormalServices>>,live:LiveAdmission<'w,'s>,geometry:Geometry<'w,'s>,
}
impl Commits<'_,'_> {
    pub(crate) fn discard(&mut self,token:u64) {
        if let Some(s)=self.state.as_mut() {
            if s.armed.as_ref().is_some_and(|a|a.token==token){s.armed=None;}
        }
    }
    pub(crate) fn resolve(&mut self,token:u64,current:&Guard,game:&fnv_game::Game<'_>,enabled:bool)->Option<Action> {
        let s=self.state.as_mut()?;
        if s.armed.as_ref()?.token!=token{return None}
        let a=s.armed.take()?;
        if !enabled || a.epoch!=s.epoch || s.hidden || !s.visible()
            || s.stamp!=Some(a.tap.stamp) || s.frame!=Some(a.tap.stamp.frame)
            || a.tap.stamp.text_size!=self.geometry.size(current.kind())
            || !self.live.allows(a.tap.stamp.frame,a.tap.id) || self.live.creator_open() || !menu_allows(current.kind(),Some(game)){return None}
        if matches!(a.tap.hit.action,Action::Browse(..)) {
            return a.tap.guard.same_session(current).then_some(a.tap.hit.action)
        }
        let source_matches = |actual: Source, expected: Source| {
            actual == expected
                || matches!((actual, expected), (Source::Tile(m1, t1), Source::Direct(m2, t2)) if m1 == m2 && t1 == t2)
        };
        if a.tap.swipe {
            if !a.tap.guard.same(current) || !self.geometry.hit(Some(game),a.tap.stamp.frame,current,a.tap.origin)
                .is_some_and(|h|source_matches(h.source,a.tap.hit.source) && h.action==Action::CategoryHeader) {return None}
            return Some(a.tap.hit.action);
        }
        if !a.tap.guard.same(current) || !self.geometry.hit(Some(game),a.tap.stamp.frame,current,a.tap.point)
            .is_some_and(|h|source_matches(h.source,a.tap.hit.source) && same_action(h.action,a.tap.hit.action)){return None}
        Some(expected(current,a.tap.hit.action))
    }
}

fn same_action(a:Action,b:Action)->bool {
    matches!((a,b),(Action::Amount(super::model::Command::Amount(_)),Action::Amount(super::model::Command::Amount(_)))) || a==b
}

/// A non-action list area, using the same current clipped native candidates.
/// This also permits browsing the recipe ingredients whose rows intentionally
/// have no normal pointer action. It never makes them actionable.
pub(crate) fn native_pane(ui:&fnv_ui::Ui,frame:Frame,list:fnv_ui::TileId,pane:usize,point:Vec2)->Option<Hit> {
    let menu=ui.root_of(list);
    let at=crate::tiles::input::to_menu_units(point,frame.size,Vec2::splat(0.5));
    let candidates=fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure);
    for c in candidates.iter().rev() {
        let r=c.region();
        if r.width<=0.0 || r.height<=0.0 || at.0<r.left || at.0>=r.left+r.width
            || at.1<r.top || at.1>=r.top+r.height {continue}
        let mut ancestor=Some(c.under);
        while let Some(node)=ancestor {
            if node==list {return Some(Hit {source:Source::Tile(menu,list),action:Action::Pane(pane),pane:Some(pane),row_step:0.0})}
            ancestor=ui.tile(node).parent;
        }
    }
    None
}

/// Native feed lends its actual Game and already-owned native tile states. Do
/// not put ScriptState or those mutable tile resources into this parameter.
#[derive(SystemParam)]
pub(crate) struct NativeAdmission<'w,'s> {
    live:LiveAdmission<'w,'s>,
    preferences:Option<Res<'w,crate::maintenance_touch::State>>,
    readability:Option<Res<'w,crate::menu_readability::MenuReadability>>,
}
impl NativeAdmission<'_,'_> {
    pub(crate) fn allows(&self,n:NativeTap,game:&fnv_game::Game<'_>)->bool {
        self.preferences.as_ref().is_some_and(|p|p.enabled()) && self.live.allows(n.frame,n.id) && !self.live.creator_open()
            && menu_allows(n.kind,Some(game))
            && n.text_size==super::readability::size(self.readability.as_deref(),match n.kind {
                Kind::Vendor=>crate::menu_readability::MenuKind::Repair,
                Kind::Workbench=>crate::menu_readability::MenuKind::Workbench,
            })
    }
}

fn dispatch(
    mut state:ResMut<NormalServices>,sources:Sources,live:LiveAdmission,
    mut script:Option<ResMut<crate::script::ScriptState>>,
    (mut vendor,mut recipe):(Option<ResMut<crate::tiles::repair_services_menu::RepairServicesTiles>>,
        Option<ResMut<crate::tiles::recipe_menu::RecipeTiles>>),
    (mut vendor_out,mut recipe_out):(MessageWriter<crate::repair::RepairServicesInput>,MessageWriter<crate::crafting::RecipeInput>),
    readability:Option<Res<crate::menu_readability::MenuReadability>>,
) {
    if state.scroll.is_none() && state.pending.is_none(){return}
    let Some(kind)=state.kind else{return};
    let text_size=super::readability::size(readability.as_deref(),match kind {
        Kind::Vendor=>crate::menu_readability::MenuKind::Repair,Kind::Workbench=>crate::menu_readability::MenuKind::Workbench});
    if let Some(scroll)=state.scroll.take() {
        let game=script.as_ref().and_then(|s|s.rt.as_ref()).map(|r|&r.game);
        if state.stamp==Some(scroll.stamp) && scroll.stamp.text_size==text_size
            && state.frame==Some(scroll.stamp.frame) && sources.same_session(&scroll.guard)
            && live.allows(scroll.stamp.frame,scroll.id) && !live.creator_open() && menu_allows(kind,game) {
            if scroll.native {
                if let Some(rt)=script.as_mut().and_then(|s|s.rt.as_mut()) {
                    match kind {
                        Kind::Vendor=>if let Some(v)=vendor.as_mut(){v.normal_scroll(&mut rt.game.ui_tree,scroll.pixels,scroll.stamp.frame,text_size);},
                        Kind::Workbench=>if let Some(v)=recipe.as_mut(){v.normal_scroll(&mut rt.game.ui_tree,scroll.pane,scroll.pixels,scroll.stamp.frame,text_size);},
                    }
                }
            }
            // Native rows have already moved in their own view; zero only
            // publishes the owner's ordinary pointer-browse intent. Hand-built
            // rows advance through the same owner with their measured row step.
            state.pending=Some(Tap {id:scroll.id,stamp:scroll.stamp,origin:scroll.point,point:scroll.point,
                hit:Hit {source:scroll.source,action:Action::Browse(scroll.pane,if scroll.native {0}else{scroll.rows}),pane:Some(scroll.pane),row_step:0.0},
                swipe:false,guard:scroll.guard});
        }
    }
    let Some(tap)=state.pending.take() else{return};
    let game=script.as_ref().and_then(|s|s.rt.as_ref()).map(|r|&r.game);
    if state.stamp!=Some(tap.stamp) || tap.stamp.text_size!=text_size || !state.visible()
        || !live.allows(tap.stamp.frame,tap.id) || live.creator_open() || !menu_allows(kind,game){return}
    state.next=state.next.wrapping_add(1).max(1);let token=state.next;let epoch=state.epoch;
    let native=matches!(tap.hit.source,Source::Tile(..)) && !tap.swipe && !matches!(tap.hit.action,Action::Browse(..));
    let native_tap=NativeTap {token,id:tap.id,frame:tap.stamp.frame,source:tap.hit.source,
        point:tap.point,action:tap.hit.action,kind,text_size};
    state.armed=Some(Armed {token,epoch,tap});
    if native {state.native=Some(native_tap);}
    else {match kind {
        Kind::Vendor=>{vendor_out.write(crate::repair::RepairServicesInput::CommitNormal(token));}
        Kind::Workbench=>{recipe_out.write(crate::crafting::RecipeInput::CommitNormal(token));}
    }}
}

#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct Dispatch;
pub(super) fn install(app:&mut App) {
    app.init_resource::<NormalServices>().add_message::<crate::repair::RepairServicesInput>()
        .add_message::<crate::crafting::RecipeInput>()
        .add_systems(Update,dispatch.in_set(Dispatch).before(crate::tiles::input::feed)
            .before(crate::repair::NormalInput).before(crate::crafting::NormalInput));
}

#[cfg(test)]
mod tests;
