//! Sole-reader adapter: admitted bottom/edge/close targets own one dismissal contact.
use bevy::{
    ecs::{message::MessageReader, system::SystemParam},
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
};
use nvf_play::{
    pipboy::{gesture::ArmGesture, Frame, Phase, PipBoy, UsePipBoy},
    settings::{self, Setting},
};

#[derive(Resource)]
pub(super) struct Options {
    pub direct: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self { direct: true }
    }
}
#[derive(Clone, Copy)]
struct Drag {
    id: u64,
    origin: Vec2,
    size: Vec2,
    start: f32,
    canceled: bool,
    vertical: bool,
    closing_travel: Option<f32>,
    distance: Option<f32>,
    deadzone: f32,
    frame: Frame,
    camera: Option<CameraStamp>,
    display: Option<Rect>,
    ui_scale: u32,
    target: Option<super::pipboy_options::Target>,
    region: Option<Rect>,
    real_arm: bool,
    safe: nvf_play::touch_safe_area::SafeInsets,
    preferences: (bool,bool,bool,u16),
    context: Option<CloseContext>,
    window: WindowStamp,
}
// An open handle is already near the bottom edge. Its downward range is
// the actual remaining safe travel, independently of the opening preference.
fn closing_travel(size: Vec2, safe_bottom: f32, origin: Vec2) -> Option<f32> {
    if !size.is_finite()
        || size.min_element() <= 0.0
        || !safe_bottom.is_finite()
        || !origin.is_finite()
        || origin.x < 0.0
        || origin.x > size.x
        || origin.y < 0.0
    {
        return None;
    }
    let travel = size.y - safe_bottom.max(0.0) - origin.y;
    (travel.is_finite() && travel > 0.0).then_some(travel)
}
#[derive(Clone, Copy)]
struct CloseContext {
    frame: Frame, page: nvf_play::pipboy::Page, tab: usize,
    image: bevy::asset::AssetId<Image>,
    scale:u32,on_model:bool, phase:Phase,
}
impl CloseContext {
    fn capture(pip: &PipBoy, screen: &nvf_play::pipboy::PipBoyScreen) -> Self {
        Self { frame:pip.frame,page:pip.page,tab:pip.tab(),image:screen.image.id(),scale:screen.scale.to_bits(),on_model:screen.on_model,phase:pip.phase }
    }
    fn matches(self, pip: &PipBoy, screen: &nvf_play::pipboy::PipBoyScreen, owned_arm:bool) -> bool {
        ((pip.phase==self.phase && (owned_arm || pip.open())) || (owned_arm && pip.phase==Phase::Raising)) && pip.frame==self.frame && pip.page==self.page && pip.tab()==self.tab && screen.image.id()==self.image && screen.scale.to_bits()==self.scale && screen.on_model==self.on_model
    }
}
// Camera identity/target/lens are stable while the normal arm animation moves
// the lens/rig. Do not compare projected screen bounds or camera translation:
// those are expected to move under this contact's own pose.
#[derive(Clone,Copy,PartialEq)]
struct CameraStamp { entity:Entity, viewport:URect, projection:Mat4 }
impl CameraStamp {
    fn of(entity:Entity,camera:&Camera,target:&bevy::camera::RenderTarget,window:WindowStamp)->Option<Self> {
        let viewport=camera.physical_viewport_rect()?;
        let projection=camera.clip_from_view();
        (camera.is_active && projection.is_finite()
            && camera.physical_target_size()==Some(window.physical)
            && matches!(target.normalize(Some(window.entity)),Some(bevy::camera::NormalizedRenderTarget::Window(w)) if w.entity()==window.entity))
            .then_some(Self {entity,viewport,projection})
    }
}
#[derive(Clone,Copy,PartialEq)]
struct WindowStamp { entity:Entity,physical:UVec2,density:u32,logical:Vec2 }
impl WindowStamp {
    fn of(entity:Entity,w:&Window)->Option<Self> {
        let logical=w.resolution.size();
        (logical.is_finite() && logical.min_element()>0.0 && w.scale_factor().is_finite() && w.scale_factor()>0.0)
            .then_some(Self {entity,physical:UVec2::new(w.physical_width(),w.physical_height()),density:w.scale_factor().to_bits(),logical})
    }
}
struct PendingClose {
    id:u64,start:Vec2,display:Option<Rect>,ui_scale:u32,context:CloseContext,preferences:(bool,bool,bool,u16),safe:nvf_play::touch_safe_area::SafeInsets,size:Vec2,region:Rect,window:WindowStamp,
}
type Regions=Vec<(super::pipboy_options::Target,Rect)>;
type PaintState=(Regions,bool,Option<super::pipboy_options::PaintFrame>);
#[derive(Resource, Default)]
pub(super) struct State {
    ids: [Option<u64>;32],
    overflow: bool,
    drag: Option<Drag>,
    pending: Option<PendingClose>,
    regions: Regions,
    display: Option<Rect>,
    admitted: bool,
}

impl State {
    fn block_id(&mut self,id:u64) {
        if self.ids.contains(&Some(id)) {return;}
        if let Some(slot)=self.ids.iter_mut().find(|s|s.is_none()) {*slot=Some(id);} else {self.overflow=true;}
    }
    /// Called by the existing post-selector UI owner after its begin_frame. No
    /// raw event reader is added; actual Touches only establishes full neutral.
    pub(super) fn publish_mask(&mut self,mask:&mut nvf_play::ui_pointer::TouchUiCapture,touches:&bevy::input::touch::Touches) {
        for id in self.ids.iter().flatten() {mask.block(*id);}
        if self.overflow {mask.quarantine_overflow();}
        if touches.iter().next().is_none() && !touches.any_just_released() && !touches.any_just_canceled() {
            self.ids.fill(None);self.overflow=false;
        }
    }
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w, 's> {
    state: ResMut<'w, State>,
    gesture: ResMut<'w, ArmGesture>,
    options: Res<'w, Options>,
    optional: Res<'w, super::pipboy_options::Options>,
    insets: Option<Res<'w, nvf_play::touch_safe_area::SafeInsets>>,
    screen: Option<Res<'w, nvf_play::pipboy::PipBoyScreen>>,
    drawer: Res<'w, super::drawer::Options>,
    safe: Res<'w, super::drawer::SafeBottom>,
    pip: Option<Res<'w, PipBoy>>,
    body: Option<Res<'w, crate::player_body::PlayerBody>>,
    use_pip: Option<Res<'w, UsePipBoy>>,
    sim: Option<Res<'w, nvf_play::sim::SimState>>,
    menu: Option<Res<'w, nvf_play::menu::ActiveMenu>>,
    prompt: Option<Res<'w, nvf_play::menu::PromptKeys>>,
    locks: Option<Res<'w, nvf_play::control::ControlLocks>>,
    commands: Commands<'w, 's>,
    windows: Query<'w,'s,(Entity,&'static Window),With<bevy::window::PrimaryWindow>>,
    cameras: Query<'w,'s,(Entity,&'static Camera,&'static bevy::camera::RenderTarget),With<nvf_play::PlayerCamera>>,
    ui_scale: Option<Res<'w,UiScale>>,
}
impl Bridge<'_, '_> {
    fn ui_scale(&self)->Option<u32> {
        let scale=self.ui_scale.as_ref().map_or(1.0,|s|s.0);
        (scale.is_finite() && scale>0.0 && (48.0/scale).is_finite()).then_some(scale.to_bits())
    }
    fn camera(&self,window:WindowStamp)->Option<CameraStamp> {
        let (entity,camera,target)=self.cameras.single().ok()?;
        CameraStamp::of(entity,camera,target,window)
    }

    /// Parent filters whole rectangles against the actual current painted model/flat canvas and tray.
    pub(super) fn prepare_regions(&mut self,size:Vec2,display:Option<Rect>,mut unclaimed:impl FnMut(Rect)->bool) {
        self.state.regions.clear();self.state.display=None;
        if !self.state.admitted || !self.inventory() {return;}
        let Some(display)=display.filter(|r|r.min.is_finite() && r.max.is_finite() && r.size().min_element()>0.0) else {return;};
        self.state.display=Some(display);
        let safe=self.insets.as_deref().copied().unwrap_or_default();
        self.state.regions=self.optional.regions(size,safe).into_iter().filter(|(target,r)| {
            let matching_side=match target {super::pipboy_options::Target::Left=>r.center().x<display.center().x,super::pipboy_options::Target::Right=>r.center().x>=display.center().x,super::pipboy_options::Target::Close=>true};
            matching_side && unclaimed(*r)
        }).collect();
    }
    pub(super) fn captures(&self) -> bool {
        self.gesture.captures() || self.state.drag.is_some() || self.state.pending.is_some() || self.state.overflow
    }
    pub(super) fn inventory(&self) -> bool {
        self.menu.as_deref().is_some_and(|m| {
            m.0.as_ref()
                .is_some_and(|m| m.mode == nvf_play::menu::model::Mode::Inventory)
        }) && self
            .pip
            .as_deref()
            .is_some_and(|p| p.phase == Phase::Up || self.captures())
    }
    fn block(&mut self) {
        self.commands.queue(|world: &mut World| {
            if let Some(mut gate) = world.get_resource_mut::<nvf_play::pad::PlatformInputGate>() {
                gate.set_arm_blocked(true);
            }
        });
    }
    pub(super) fn cancel(&mut self) {
        self.state.drag = None;
        self.state.pending = None;
        self.state.admitted = false;
        self.state.regions.clear();self.state.display=None;
        self.gesture.abort();
        if self.captures() {
            self.block();
        }
    }
    /// The parent supplies lifecycle/modal/device admission before draining its reader.
    pub(super) fn prepare(
        &mut self,
        allowed: bool,
        gate: Option<&nvf_play::pad::PlatformInputGate>,
    ) {
        self.state.admitted=allowed;
        if !self.captures() { self.state.drag=None; return; }
        let safe=self.insets.as_deref().copied().unwrap_or_default();
        let invalid_contact=self.state.drag.is_some_and(|d| {
            d.safe!=safe || d.preferences!=self.optional.policy()
                || self.windows.single().ok().and_then(|(e,w)|WindowStamp::of(e,w))!=Some(d.window)
                || (d.target.is_none() && (!self.options.direct || !self.drawer.enabled || self.drawer.destination!=super::drawer::Destination::Inventory))
                || d.closing_travel.is_some_and(|travel| closing_travel(d.size,self.safe.0.max(d.safe.bottom),d.origin)!=Some(travel))
                || self.ui_scale()!=Some(d.ui_scale)
                || (!d.real_arm && d.display!=self.state.display)
                || self.pip.as_deref().is_none_or(|p|p.frame!=d.frame)
                || d.camera.is_some_and(|c|self.camera(d.window)!=Some(c))
                || d.context.is_some_and(|c|self.pip.as_deref().zip(self.screen.as_deref()).is_none_or(|(p,s)|!c.matches(p,s,d.real_arm)))
        });
        let real=self.gesture.captures();
        let invalid_rig=real && (self.body.as_deref().and_then(crate::player_body::PlayerBody::first_person_root)!=self.gesture.model()
            || self.body.as_deref().is_some_and(crate::player_body::PlayerBody::pipboy_clip_missing)
            || self.use_pip.as_deref().is_none_or(|u|!u.on)
            || self.sim.as_deref().is_none_or(|s|!nvf_play::pipboy::wears_pipboy(s)));
        if !allowed || invalid_contact || invalid_rig || self.locks.as_ref().is_some_and(|l|l.pipboy)
            || gate.is_none_or(nvf_play::pad::PlatformInputGate::native_blocked) {self.cancel();}
    }
    /// Caller must first arbitrate native bottom buttons, tray/editor and scrollbar.
    pub(super) fn begin(
        &mut self,
        event: &TouchInput,
        size: Vec2,
        unclaimed: bool,
        gate: Option<&nvf_play::pad::PlatformInputGate>,
    ) -> bool {
        if self.captures() || !unclaimed || self.locks.as_ref().is_some_and(|l|l.pipboy)
            || event.phase!=TouchPhase::Started || !event.position.is_finite()
            || gate.is_none_or(|g|!g.allows_touch(event.id)) {return false;}
        let Some(ui_scale)=self.ui_scale() else {return false;};
        let Some(window)=self.windows.single().ok().filter(|(_,w)|w.focused).and_then(|(e,w)|WindowStamp::of(e,w)).filter(|w|w.logical==size && w.entity==event.window) else {return false;};
        let Some(pip)=self.pip.as_deref().filter(|p|p.warming.is_none() && matches!(p.phase,Phase::Down|Phase::Up)) else {return false;};
        let open=pip.phase==Phase::Up;
        let safe=self.insets.as_deref().copied().unwrap_or_default();
        if [safe.left,safe.top,safe.right,safe.bottom].iter().any(|v|!v.is_finite() || *v<0.0) {return false;}
        if event.position.x<safe.left || event.position.x>size.x-safe.right || event.position.y<safe.top || event.position.y>size.y-safe.bottom.max(self.safe.0) {return false;}
        let region=open.then(||self.state.regions.iter().find(|(t,r)|*t==super::pipboy_options::Target::Close && r.contains(event.position)).copied()).flatten();
        let target=region.map(|(t,_)|t).or_else(|| {
            let display=self.state.display.filter(|_|open)?;
            if event.position.x<display.center().x {self.optional.left.then_some(super::pipboy_options::Target::Left)}
            else {self.optional.right.then_some(super::pipboy_options::Target::Right)}
        });
        if target.is_none() && (!self.options.direct || !self.drawer.enabled || self.drawer.destination!=super::drawer::Destination::Inventory
            || !(if open {super::drawer::handle(size,self.safe.0)} else {super::drawer::opening_area(size,self.safe.0,safe)}).contains(event.position)) {return false;}
        let model=self.body.as_deref().and_then(crate::player_body::PlayerBody::first_person_root);
        let real_arm=pip.frame==Frame::Device && self.options.direct && model.is_some()
            && !self.body.as_deref().is_some_and(crate::player_body::PlayerBody::pipboy_clip_missing)
            && self.use_pip.as_deref().is_some_and(|u|u.on)
            && self.sim.as_deref().is_some_and(nvf_play::pipboy::wears_pipboy);
        if !real_arm && (!open || target.is_none()) {return false;}
        let menu_open = self.menu.as_deref().is_some_and(|m| {
            m.0.as_ref()
                .is_some_and(|m| m.mode == nvf_play::menu::model::Mode::Inventory)
        });
        if (pip.phase == Phase::Up) != menu_open
            || self
                .prompt
                .as_deref()
                .zip(self.menu.as_deref())
                .is_some_and(|(p, m)| p.held(m))
        {
            return false;
        }
        let closing_travel = if open {
            let Some(travel) = closing_travel(size, self.safe.0.max(safe.bottom), event.position) else {
                return false;
            };
            Some(travel)
        } else {
            None
        };
        let side=matches!(target,Some(super::pipboy_options::Target::Left|super::pipboy_options::Target::Right));
        let distance=closing_travel.map(|available|if side {self.optional.travel().min(available)} else {available});
        let deadzone=if side {distance.map_or(0.0,|distance|8.0_f32.min(distance*0.25))} else {0.0};
        let camera=if real_arm {let Some(camera)=self.camera(window) else {return false;};Some(camera)} else {None};
        if real_arm && !self.gesture.begin(open, model.expect("real arm has a model")) {
            return false;
        }
        self.state.drag = Some(Drag {
            id: event.id,
            origin: event.position,
            size,
            start: if open { 1.0 } else { 0.0 },
            canceled: false,vertical:false,
            closing_travel,distance,deadzone,frame:pip.frame,camera,display:self.state.display,ui_scale,
            target, region:region.map(|(_,r)|r).or_else(|| {
                let display=self.state.display?;
                match target {Some(super::pipboy_options::Target::Left)=>Some(Rect::from_corners(Vec2::new(safe.left,safe.top),Vec2::new(display.center().x,size.y-safe.bottom.max(self.safe.0)))),Some(super::pipboy_options::Target::Right)=>Some(Rect::from_corners(Vec2::new(display.center().x,safe.top),size-Vec2::new(safe.right,safe.bottom.max(self.safe.0)))),_=>None}
            }),real_arm, safe, preferences:self.optional.policy(),window,
            context:if open { self.screen.as_deref().map(|s|CloseContext::capture(pip,s)) } else {None},
        });
        if !real_arm && self.state.drag.is_some_and(|d|d.context.is_none()) {self.state.drag=None;return false;}
        self.state.block_id(event.id);
        if real_arm {self.block();}
        true
    }
    pub(super) fn event(&mut self, event: &TouchInput, size: Vec2) {
        if self.captures() {self.state.block_id(event.id);}
        let Some(mut drag) = self.state.drag.filter(|d| d.id == event.id) else {
            return;
        };
        let delta = event.position - drag.origin;
        drag.canceled |= self.windows.single().ok().and_then(|(e,w)|WindowStamp::of(e,w))!=Some(drag.window);
        drag.canceled |= drag.safe!=self.insets.as_deref().copied().unwrap_or_default() || drag.preferences!=self.optional.policy();
        if !drag.vertical {
            drag.canceled |= super::drawer::horizontal_intent(delta);
            drag.vertical=super::drawer::vertical_intent(delta);
        }
        drag.canceled |= drag.size != size || !event.position.is_finite()
            || event.position.x<drag.safe.left || event.position.x>size.x-drag.safe.right
            || event.position.y<drag.safe.top || event.position.y>size.y-drag.safe.bottom.max(self.safe.0);
        // A changing inset invalidates the captured safe range, just as resize
        // already invalidates its window. Opening keeps its original policy.
        drag.canceled |= drag.closing_travel.is_some()
            && closing_travel(size, self.safe.0.max(drag.safe.bottom), drag.origin) != drag.closing_travel;
        drag.canceled |= self.ui_scale()!=Some(drag.ui_scale) || (!drag.real_arm && drag.display!=self.state.display)
            || self.pip.as_deref().is_none_or(|p|p.frame!=drag.frame)
            || drag.camera.is_some_and(|c|self.camera(drag.window)!=Some(c))
            || drag.context.is_some_and(|c|self.pip.as_deref().zip(self.screen.as_deref()).is_none_or(|(p,s)|!c.matches(p,s,drag.real_arm)));
        let close_tap=drag.target==Some(super::pipboy_options::Target::Close);
        drag.canceled|=close_tap && delta.length()>12.0;
        if drag.real_arm {
            if drag.canceled {self.gesture.cancel();}
            else if !close_tap {
                let travel=drag.distance.unwrap_or(self.drawer.threshold.max(48.0));
                let down=if drag.deadzone>0.0 {(delta.y-drag.deadzone).max(0.0)/(travel-drag.deadzone)} else {delta.y/travel};
                self.gesture.drag(drag.start-down);
            }
        }
        if matches!(event.phase,TouchPhase::Ended|TouchPhase::Canceled) {
            let canceled=event.phase==TouchPhase::Canceled || drag.canceled;
            if drag.real_arm {
                if canceled {self.gesture.cancel();}
                else {if close_tap {self.gesture.drag(0.0);} self.gesture.release();}
            } else if !canceled && (close_tap || {
                let travel=drag.distance.unwrap_or(96.0);
                delta.y>=drag.deadzone+(travel-drag.deadzone)*0.5
            }) {
                self.state.pending=drag.context.zip(drag.region).map(|(context,region)|PendingClose {id:event.id,start:drag.origin,display:drag.display,ui_scale:drag.ui_scale,context,preferences:drag.preferences,safe:drag.safe,size:drag.size,region,window:drag.window});
            }
            self.state.drag=None;
        } else {self.state.drag=Some(drag);}
    }
    /// Active owner only; other contacts remain quarantined by the shared gate.
    pub(super) fn capture_batch(
        &mut self,
        events: &mut MessageReader<TouchInput>,
        window: Entity,
        size: Vec2,
    ) -> bool {
        if !self.captures() {
            return false;
        }
        for event in events.read() {
            if event.window == window {
                self.event(event, size);
            }
        }
        if self.gesture.captures() {self.block();}
        true
    }
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<super::pipboy_options::Options>()
        .init_resource::<Options>()
        .init_resource::<State>()
        .init_resource::<ArmGesture>()
        .add_message::<nvf_play::menu::MenuInput>()
        .add_systems(Update, draw_controls.before(nvf_play::PlaySet::Ui))
        // Final authoritative palette paint follows the core Update recolour.
        .add_systems(PostUpdate, paint_controls)
        .add_systems(Update, dispatch_close.after(nvf_play::pipboy::PointerSet).after(crate::pipboy_arm::show_screen).after(nvf_play::PlaySet::Scripts).before(nvf_play::menu::MenuSystems).in_set(nvf_play::PlaySet::Ui));
    settings::add_setting(
        app,
        Setting {
            key: "touch-direct-pipboy-v1",
            group: "Controls",
            label: "Direct Pip-Boy swipe",
            note: "With the bottom swipe on, drag the Pip-Boy arm up or down directly. Release settles open or closed.",
            switches: "",
            get: |w| w.resource::<Options>().direct,
            set: |w, v| {
                w.resource_mut::<Options>().direct = v;
                w.resource_mut::<ArmGesture>().abort();
            },
            slider: None,
        },
    );
}

#[derive(SystemParam)]
struct CloseAdmission<'w,'s> {
    input: nvf_play::pipboy::touch_items::Admission<'w>,
    pip: Option<Res<'w,PipBoy>>,
    screen: Option<Res<'w,nvf_play::pipboy::PipBoyScreen>>,
    active: Option<Res<'w,nvf_play::menu::ActiveMenu>>,
    paused: Option<Res<'w,nvf_play::pause::Paused>>,
    script: Option<Res<'w,nvf_play::script::ScriptState>>,
    creator: Option<Res<'w,nvf_play::creator::CreatorMenu>>,
    physical: Option<Res<'w,nvf_play::pipboy::touch_scroll::PhysicalPriority>>,
    windows: Query<'w,'s,(Entity,&'static Window),With<bevy::window::PrimaryWindow>>,
    safe: Option<Res<'w,nvf_play::touch_safe_area::SafeInsets>>,
    locks: Option<Res<'w,nvf_play::control::ControlLocks>>,
    play: Option<Res<'w,bevy::prelude::State<nvf_play::PlayState>>>,
    repair: Option<Res<'w,nvf_play::pipboy::repair::PipRepair>>,
    modding: Option<Res<'w,nvf_play::pipboy::item_mod::PipItemMod>>,
    data: Option<Res<'w,nvf_play::pipboy::data::DataState>>,
    model: super::pipboy_model::Bridge<'w,'s>,
    ui_scale: Option<Res<'w,UiScale>>,
}
fn dispatch_close(mut state:ResMut<State>, admission:CloseAdmission, optional:Res<super::pipboy_options::Options>, mut out:MessageWriter<nvf_play::menu::MenuInput>) {
    let Some(PendingClose {id,start,display,ui_scale,context,preferences,safe,size,region,window})=state.pending.take() else {return;};
    if !state.admitted || !admission.input.allows_contact(Some(id))
        || admission.paused.as_ref().is_some_and(|p|p.0)
        || admission.physical.as_ref().is_some_and(|p|p.active)
        || admission.creator.as_ref().is_some_and(|c|c.is_open())
        || !admission.windows.single().is_ok_and(|(entity,w)|w.focused && w.resolution.size()==size && WindowStamp::of(entity,w)==Some(window))
        || admission.safe.as_deref().copied().unwrap_or_default()!=safe
        || admission.locks.as_ref().is_some_and(|l|l.pipboy)
        || admission.play.as_ref().is_some_and(|s|*s.get()!=nvf_play::PlayState::Menu)
        || admission.repair.as_ref().is_some_and(|r|r.is_open())
        || admission.modding.as_ref().is_some_and(|m|m.is_open())
        || admission.data.as_ref().is_some_and(|d|d.has_popup())
        || admission.script.as_ref().is_some_and(|s|s.loading() || s.rt.as_ref().is_some_and(|rt|rt.game.loading || matches!(rt.game.side_menu(),Some(fnv_game::game::menus::LOADING|fnv_game::game::menus::VATS)) || rt.game.top_menu().is_some_and(|code|code!=fnv_game::game::menus::INVENTORY)))
        || !admission.active.as_ref().and_then(|a|a.0.as_ref()).is_some_and(|m|m.mode==nvf_play::menu::model::Mode::Inventory && m.quantity.is_none())
        || admission.pip.as_deref().zip(admission.screen.as_deref()).is_none_or(|(p,s)|!context.matches(p,s,false))
        || admission.ui_scale.as_ref().map_or(1.0,|s|s.0).to_bits()!=ui_scale
        || display!=admission.model.dismissal_display()
        || optional.policy()!=preferences || !region.contains(start) || !admission.model.closing_unclaimed(start) {return;}
    out.write(nvf_play::menu::MenuInput(nvf_play::menu::model::Command::Close));
}

#[derive(Component)]
struct OptionalVisual;
#[derive(Component)]
struct OptionalCaption;
fn draw_controls(mut commands:Commands,state:Res<State>,pip:Option<Res<PipBoy>>,canvas:super::pipboy_options::OverlayCanvas,old:Query<Entity,With<OptionalVisual>>,mut previous:Local<Option<PaintState>>) {
    let active=state.admitted && pip.as_ref().is_some_and(|p|p.phase==Phase::Up);
    let frame=canvas.frame();
    let next=(state.regions.clone(),active,frame);
    if previous.as_ref()==Some(&next) {return;} *previous=Some(next);
    for entity in &old {commands.entity(entity).despawn();}
    let Some(frame)=frame.filter(|_|active) else {return;};
    for (target,rect) in &state.regions {
        let label=if *target==super::pipboy_options::Target::Close {"Close"} else {"Drag down"};
        commands.spawn((OptionalVisual,UiTargetCamera(frame.camera),Node {position_type:PositionType::Absolute,left:px(rect.min.x/frame.scale),top:px(rect.min.y/frame.scale),width:px(rect.width()/frame.scale),height:px(rect.height()/frame.scale),border:UiRect::all(px(1.0/frame.scale)),align_items:AlignItems::Center,justify_content:JustifyContent::Center,..default()},
            BackgroundColor(Color::srgba(0.02,0.025,0.02,0.2)),BorderColor::all(nvf_play::interface::pipboy_amber(255.0).with_alpha(0.65)),GlobalZIndex(1800),Pickable::IGNORE))
            .with_children(|p| {p.spawn((OptionalCaption,Text::new(label),TextFont {font_size:FontSize::Px(12.0/frame.scale),..default()},TextColor(nvf_play::interface::pipboy_amber(255.0)),Pickable::IGNORE));});
    }
}

fn paint_controls(options:Res<super::pipboy_options::Options>,mut nodes:Query<(&mut BackgroundColor,&mut BorderColor),With<OptionalVisual>>,mut captions:Query<&mut TextColor,With<OptionalCaption>>) {
    let alpha=if options.graphics {1.0} else {0.0};
    for (mut background,mut border) in &mut nodes {
        background.0=Color::srgba(0.02,0.025,0.02,0.2*alpha);
        *border=BorderColor::all(nvf_play::interface::pipboy_amber(255.0).with_alpha(0.65*alpha));
    }
    for mut caption in &mut captions {caption.0=nvf_play::interface::pipboy_amber(255.0).with_alpha(alpha);}
}

#[cfg(test)]
pub(super) mod tests;
