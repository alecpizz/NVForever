//! Field Notes side framing and the supplied Classic live-world viewport lease.
use bevy::{camera::{visibility::RenderLayers, CameraOutputMode, RenderTarget, Viewport}, prelude::*, render::render_resource::BlendState, ui::{DefaultUiCamera,IsDefaultUiCamera}, window::PrimaryWindow};
use crate::{dialogue_menu::DialogueMenu, dialogue_touch::TouchDialogue, PlayerCamera};
use crate::touch_safe_area::SafeInsets;

pub const KEY: &str = "dialogue-speaker-side";
pub const DEVICE_SIDE_KEY: &str = "dialogue-device-side-v1";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side { #[default] Standard, Left, Right }
#[cfg(any(target_os = "android", test))]
impl Side {
    fn name(self) -> &'static str { match self { Self::Standard => "Standard", Self::Left => "Speaker left", Self::Right => "Speaker right" } }
    fn named(name: &str) -> Option<Self> { [Self::Standard, Self::Left, Self::Right].into_iter().find(|s| s.name() == name) }
}
/// [design: docs/design/android.md] Explicit Left/Right side placement for Wrist
/// Terminal and Retro Console dialogue models (default Right, framing the NPC left).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceSide { #[default] Right, Left }
impl DeviceSide {
    #[cfg(any(target_os = "android", test))]
    fn name(self) -> &'static str { match self { Self::Right => "Right", Self::Left => "Left" } }
    #[cfg(any(target_os = "android", test))]
    fn named(name: &str) -> Option<Self> { [Self::Right, Self::Left].into_iter().find(|s| s.name() == name) }
    fn speaker_side(self) -> Side { match self { Self::Right => Side::Left, Self::Left => Side::Right } }
}
#[cfg_attr(test, derive(Debug))]
#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub struct Options { pub side: Side, pub device_side: DeviceSide }
impl Default for Options {
    fn default() -> Self {
        Self {
            side: if cfg!(target_os="android") { Side::Left } else { Side::Standard },
            device_side: DeviceSide::Right,
        }
    }
}
impl Options {
    /// Side of the speaker for device-model dialogue (Wrist Terminal / Retro Console).
    /// Always docks cleanly to Left or Right even when Field Notes speaker side is Standard.
    pub(crate) fn device_speaker_side(self) -> Side {
        match self.side {
            Side::Left | Side::Right => self.side,
            Side::Standard => self.device_side.speaker_side(),
        }
    }
}
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Frame {
    pub panel: Option<Rect>,
    speaker: Option<Rect>,
    pub ui_camera: Option<Entity>,
    pub classic: Option<ClassicViewport>,
}

/// Current physical world opening, included in the raw-touch frame identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClassicViewport {
    pub window: Entity,
    pub camera: Entity,
    pub prepared:crate::dialogue_touch::classic_presenter::Prepared,
    pub world: Rect,
    pub window_size: Vec2,
    pub density: f32,
    pub ui_scale: f32,
    pub physical_position: UVec2,
    pub physical_size: UVec2,
}
impl ClassicViewport {
    fn viewport(self) -> Viewport {Viewport {physical_position:self.physical_position,
        physical_size:self.physical_size,..default()}}
}
#[derive(Resource,Default)]
struct ViewportLease {owned:Option<OwnedViewport>, blocked:Option<Entity>}
/// Proposals are private until the actual camera write succeeds. Painters never
/// select Classic using a Resolve-only candidate.
#[derive(Resource,Default)]
struct ClassicRequest(Option<ClassicViewport>);
struct OwnedViewport {entity:Entity,written:Viewport}
fn same_viewport(a:&Viewport,b:&Viewport)->bool {a.physical_position==b.physical_position
    && a.physical_size==b.physical_size && a.depth==b.depth}
fn owns_viewport(lease:&ViewportLease,entity:Entity,camera:&Camera)->bool {
    lease.owned.as_ref().is_some_and(|l|l.entity==entity
        && camera.viewport.as_ref().is_some_and(|v|same_viewport(v,&l.written)))
}

fn classic_viewport(surface:&crate::dialogue_touch::classic_presenter::Prepared,camera:Entity,window:&Window)->Option<ClassicViewport> {
    let world=Rect::from_corners(surface.layout.world.min+surface.origin,surface.layout.world.max+surface.origin);
    let min=(world.min*surface.density).floor();let max=(world.max*surface.density).ceil();
    let size=Vec2::new(window.physical_width() as f32,window.physical_height() as f32);
    if !min.is_finite() || !max.is_finite() || !min.cmpge(Vec2::ZERO).all()
        || !max.cmple(size).all() || !(max-min).cmpge(Vec2::ONE).all() {return None;}
    Some(ClassicViewport {window:surface.window,camera,prepared:*surface,world,window_size:surface.size,
        density:surface.density,ui_scale:surface.ui_scale,physical_position:min.as_uvec2(),physical_size:(max-min).as_uvec2()})
}
#[derive(Resource, Default)]
struct Target(Option<Vec3>);
#[derive(Component)]
/// Full-window UI camera for active enhanced dialogue, including fallback views.
pub struct OverlayCamera;
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Resolve;
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Admit;
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Validate;
#[derive(Component)]
struct OwnedDefault;
#[derive(Resource,Default)]
struct DefaultPin(Option<Entity>);
#[derive(Resource, Default)]
struct Lease(Option<Owned>);
struct Owned {
    entity: Entity,
    base_rotation: Quat,
    written_rotation: Quat,
}

#[cfg(any(target_os = "android", test))]
fn chosen(w: &World) -> String { w.resource::<Options>().side.name().into() }
#[cfg(any(target_os = "android", test))]
fn choose(w: &mut World, name: &str) -> bool {
    let Some(side) = Side::named(name) else { return false };
    let mut opts = w.resource_mut::<Options>();
    opts.side = side;
    match side {
        Side::Left => opts.device_side = DeviceSide::Right,
        Side::Right => opts.device_side = DeviceSide::Left,
        Side::Standard => {}
    }
    true
}
#[cfg(any(target_os = "android", test))]
fn chosen_device_side(w: &World) -> String { w.resource::<Options>().device_side.name().into() }
#[cfg(any(target_os = "android", test))]
fn choose_device_side(w: &mut World, name: &str) -> bool {
    let Some(device_side) = DeviceSide::named(name) else { return false };
    let mut opts = w.resource_mut::<Options>();
    opts.device_side = device_side;
    if opts.side != Side::Standard {
        opts.side = device_side.speaker_side();
    }
    true
}
pub(crate) fn install(app: &mut App) {
    app.init_resource::<Options>().init_resource::<SafeInsets>().init_resource::<Frame>()
        .init_resource::<Target>().init_resource::<Lease>().init_resource::<DefaultPin>().init_resource::<ViewportLease>().init_resource::<ClassicRequest>()
        .add_systems(Update, validate_existing.in_set(Validate).after(crate::PlaySet::Input).before(crate::PlaySet::Scripts))
        .add_systems(Update, resolve.in_set(Resolve).after(crate::PlaySet::Scripts).before(crate::PlaySet::Ui))
        .add_systems(Update, apply_classic.in_set(Admit).after(Resolve).before(crate::PlaySet::Ui))
        .add_systems(Update, apply.in_set(crate::PlaySet::Sync).after(crate::control::place_camera).after(crate::dialogue_zoom::apply));
    #[cfg(any(target_os = "android", test))]
    {
        crate::settings::add_choice_list(app, crate::settings::Setting {
            key:KEY, group:crate::pause::GROUP_MENUS, label:"Dialogue speaker side",
            note:"With touch dialogue, Field Notes, Retro Console and Wrist Terminal sit on one side and frame the speaker on the other. Standard keeps the speaker centered; Classic keeps its own framing.",
            switches:"", get:|_|false,set:|_,_|{},slider:None,
        },crate::settings::ChoiceList { names:|_|[Side::Standard,Side::Left,Side::Right].map(|s|s.name().into()).to_vec(),
            chosen,pick:choose,save:chosen,load:choose,ask_keep:false });
        crate::settings::add_choice_list(app, crate::settings::Setting {
            key:DEVICE_SIDE_KEY, group:crate::pause::GROUP_MENUS, label:"Device dialogue side",
            note:"Place the Wrist Terminal or Retro Console on the Right (default) or Left side of the screen, framing the speaker on the opposite side.",
            switches:"", get:|_|false,set:|_,_|{},slider:None,
        },crate::settings::ChoiceList { names:|_|[DeviceSide::Right,DeviceSide::Left].map(|s|s.name().into()).to_vec(),
            chosen:chosen_device_side,pick:choose_device_side,save:chosen_device_side,load:choose_device_side,ask_keep:false });
    }
}

fn rectangles(size: Vec2, safe: SafeInsets, side: Side) -> Option<(Rect,Rect)> {
    if side == Side::Standard || !size.is_finite() || size.x < 480.0 || size.y < 240.0 { return None }
    let safe = SafeInsets::from_array([safe.left,safe.top,safe.right,safe.bottom])?;
    let inner = Rect::from_corners(Vec2::new(safe.left+8.0,safe.top+8.0),size-Vec2::new(safe.right+8.0,safe.bottom+8.0));
    if inner.width() < 464.0 || inner.height() < 160.0 { return None }
    let mid = (inner.min.x+inner.max.x)*0.5;
    let left = Rect::from_corners(inner.min,Vec2::new(mid-4.0,inner.max.y));
    let right = Rect::from_corners(Vec2::new(mid+4.0,inner.min.y),inner.max);
    Some(if side==Side::Left {(left,right)}else{(right,left)})
}

/// [design: docs/design/android.md] Enlarged side split for Wrist Terminal and
/// Retro Console so the device model reaches nearly to the top, bottom and chosen
/// side screen edges (66% width) while framing the speaker cleanly opposite.
fn device_panel_rectangles(size: Vec2, safe: SafeInsets, side: Side) -> Option<(Rect, Rect)> {
    if side == Side::Standard || !size.is_finite() || size.x < 480.0 || size.y < 240.0 { return None }
    let safe = SafeInsets::from_array([safe.left, safe.top, safe.right, safe.bottom])?;
    let inner = Rect::from_corners(Vec2::new(safe.left, safe.top), size - Vec2::new(safe.right, safe.bottom));
    if inner.width() < 464.0 || inner.height() < 160.0 { return None }
    let panel_share = 0.66;
    let (speaker, panel) = if side == Side::Left {
        let split = inner.min.x + inner.width() * (1.0 - panel_share);
        (Rect::from_corners(inner.min, Vec2::new(split - 2.0, inner.max.y)),
         Rect::from_corners(Vec2::new(split + 2.0, inner.min.y), inner.max))
    } else {
        let split = inner.min.x + inner.width() * panel_share;
        (Rect::from_corners(Vec2::new(split + 2.0, inner.min.y), inner.max),
         Rect::from_corners(inner.min, Vec2::new(split - 2.0, inner.max.y)))
    };
    Some((speaker, panel))
}

/// The physical window rectangle a side-placed device model is presented in.
pub(crate) fn device_display(panel: Rect, density: f32) -> Rect {
    Rect::from_corners(panel.min*density,panel.max*density)
}

type SplitResources<'w> = (
    Res<'w, Options>,
    Res<'w, SafeInsets>,
    Res<'w, TouchDialogue>,
    Res<'w, crate::dialogue_touch::appearance::Appearance>,
    Option<Res<'w, crate::dialogue_touch::classic_presenter::Surface>>,
    (Option<Res<'w, crate::dialogue_device::DeviceView>>, Option<Res<'w, crate::dialogue_device::Request>>),
);
type SceneState<'w> = (Option<Res<'w,State<crate::PlayState>>>, Option<Res<'w,crate::pause::Paused>>, Option<Res<'w,crate::free_cam::FreeCam>>, Option<Res<'w,crate::control::PlayerControl>>, Option<Res<'w,crate::player::ViewDriven>>,Option<Res<'w,crate::creator::CreatorMenu>>);
type SpeakerSources<'w> = (Option<Res<'w,crate::script::ScriptState>>, Option<Res<'w,crate::dialogue_look::SpeakerFace>>, Option<Res<'w,crate::phys::PhysState>>, Option<Res<'w,crate::sim::SimState>>);
type ResolveQueries<'w,'s> = (
    Query<'w,'s,(Entity,&'static Window),With<PrimaryWindow>>,
    Query<'w,'s,(Entity,&'static Camera,&'static Projection,&'static RenderTarget),With<PlayerCamera>>,
    DefaultUiCamera<'w,'s>,
    Query<'w,'s,Entity,(With<Camera>,With<IsDefaultUiCamera>)>,
    Query<'w,'s,Entity,With<OwnedDefault>>,
    Query<'w,'s,(Entity,&'static Camera),With<OverlayCamera>>,
    Query<'w,'s,(Entity,&'static Camera,&'static RenderTarget),Without<OverlayCamera>>,
);
type ResolveOutput<'w> = (ResMut<'w,DefaultPin>,ResMut<'w,Frame>,ResMut<'w,Target>,ResMut<'w,ClassicRequest>,ResMut<'w,ViewportLease>);
fn resolve(
    mut commands: Commands, (options,safe,touch,appearance,surface,(device,device_request)): SplitResources<'_>,
    (state,paused,free,control,driven,creator): SceneState<'_>, menu: Res<DialogueMenu>,
    (script,face,phys,sim): SpeakerSources<'_>,
    (windows,world_cameras,default_ui,default_markers,own_markers,overlays,all_cameras): ResolveQueries<'_,'_>,
    (mut pin,mut frame,mut target,mut request,mut lease): ResolveOutput<'_>,
) {
    let mut next=Frame::default();request.0=None;target.0=None;
    let foreign_change=pin.0.is_some_and(|p|!default_markers.contains(p) || default_markers.iter().any(|e|e!=p));
    if foreign_change {
        if let Some(entity)=lease.owned.as_ref().map(|l|l.entity) {lease.blocked=Some(entity);}
        if let Some(entity)=pin.0.take().filter(|e|own_markers.contains(*e)) {
            commands.entity(entity).remove::<(IsDefaultUiCamera,OwnedDefault)>();
        }
    }
    // Every touch dialogue painter shares this actual full-window UI target.
    // A model/viewport failure changes content selection, not UI availability.
    let ui_allowed=touch.enabled && menu.view.is_some() && crate::dialogue_touch::available(
        state.as_deref(),paused.as_deref(),script.as_deref(),creator.as_deref());
    if let Ok((window_entity,window))=windows.single() {
        if ui_allowed && window.width().is_finite() && window.height().is_finite()
            && window.width()>0.0 && window.height()>0.0 {
            if pin.0.is_none() && default_markers.is_empty() {
                let original=default_ui.get().filter(|e|!overlays.contains(*e)).or_else(||
                    all_cameras.iter().filter(|(_,_,t)|t.normalize(Some(window_entity))==RenderTarget::default().normalize(Some(window_entity)))
                        .max_by_key(|(e,c,_)|(c.order,*e)).map(|(e,_,_)|e));
                if let Some(entity)=original {
                    commands.entity(entity).insert((IsDefaultUiCamera,OwnedDefault));pin.0=Some(entity);
                }
            }
            let order=all_cameras.iter().filter(|(_,c,t)|c.is_active
                && t.normalize(Some(window_entity))==RenderTarget::default().normalize(Some(window_entity)))
                .map(|(_,c,_)|c.order).max().unwrap_or(0).saturating_add(1);
            let overlay=if let Ok((entity,camera))=overlays.single() {
                if camera.order!=order {
                    // Resolve is read-only over the camera queries. Change only
                    // our retained overlay's order at the Resolve→Admit flush.
                    commands.queue(move |world:&mut World| {
                        if world.get::<OverlayCamera>(entity).is_some() {
                            if let Some(mut camera)=world.get_mut::<Camera>(entity) {camera.order=order;}
                        }
                    });
                }
                entity
            } else {commands.spawn((
                OverlayCamera,Name::new("Touch dialogue overlay camera"),Camera2d,
                Camera {order,is_active:false,clear_color:ClearColorConfig::Custom(Color::NONE),
                    output_mode:CameraOutputMode::Write {blend_state:Some(BlendState::ALPHA_BLENDING),clear_color:ClearColorConfig::None},..default()},
                RenderTarget::Window(bevy::window::WindowRef::Entity(window_entity)),RenderLayers::none(),
            )).id()};
            next.ui_camera=Some(overlay);
            use crate::dialogue_touch::appearance::Theme;
            let device_kind=match appearance.theme {
                Theme::RetroConsole=>Some(crate::dialogue_device::DeviceKind::Terminal),
                Theme::WristTerminal=>Some(crate::dialogue_device::DeviceKind::PipBoy),
                _=>None,
            };
            let sides = if device_kind.is_some() {
                device_panel_rectangles(Vec2::new(window.width(), window.height()), *safe, options.device_speaker_side())
            } else {
                rectangles(Vec2::new(window.width(), window.height()), *safe, options.side)
            };
            // A device keeps its side even while camera framing is suspended, so
            // its model is not rebuilt for every cinematic or held line.
            if device_kind.is_some() {next.panel=sides.map(|(_,panel)|panel);}
            // The speaker moves aside only once that side model actually presents
            // the current request; a publication left from a replaced request does not.
            let device_presented=device_kind.is_some() && device.as_deref().zip(next.panel).zip(device_request.as_deref())
                .is_some_and(|((view,panel),request)| view.matches(request)
                    && Some(view.stamp.kind)==device_kind && view.stamp.window==window_entity
                    && view.stamp.display==device_display(panel,window.scale_factor()));
            let framing_allowed=!foreign_change && !free.is_some_and(|f|f.on)
                && !driven.is_some_and(|d|d.0) && !control.is_some_and(|c|c.cinematic || c.portrait.is_some());
            if framing_allowed {
                if let (Ok((entity,camera,projection,render_target)),Some(game))=
                    (world_cameras.single(),script.as_ref().and_then(|s|s.rt.as_ref()).map(|r|&r.game)) {
                    let primary=render_target.normalize(Some(window_entity))==RenderTarget::default().normalize(Some(window_entity));
                    if camera.is_active && primary && !crate::dialogue::held(game) {
                        if let Some(conv)=game.dialogue.as_ref() {
                            if appearance.theme==Theme::ClassicGlass
                                && matches!(projection,Projection::Perspective(_)) && lease.blocked!=Some(entity)
                                && ((lease.owned.is_none() && camera.viewport.is_none()) || owns_viewport(&lease,entity,camera)) {
                                if let Some(prepared)=surface.as_deref().and_then(|s|s.0.as_ref()).filter(|p|
                                    p.window==window_entity && p.size==Vec2::new(window.width(),window.height())
                                        && p.density==window.scale_factor() && window.focused) {
                                    if camera.computed.target_info.as_ref().is_some_and(|info|
                                        info.physical_size==UVec2::new(window.physical_width(),window.physical_height())
                                            && info.scale_factor==window.scale_factor()) {
                                        request.0=classic_viewport(prepared,entity,window);
                                    }
                                }
                            } else if (appearance.theme==Theme::SplitPanel || device_presented) && camera.viewport.is_none() {
                                let actor=conv.through.unwrap_or(conv.actor);
                                let volume=phys.as_ref().and_then(|p|p.world.actor_volume(actor.0)).map(|v|(v.feet,v.height));
                                let reference=sim.as_ref().and_then(|s|s.world.peek(actor).map(|r|r.pos).or_else(||s.refs.get(actor).map(|r|r.pos)));
                                let point=if conv.through.is_some() {reference}else {
                                    crate::dialogue_look::face_point(actor.0,face.as_ref().and_then(|f|f.0),volume,reference).map(|p|p.0)
                                };
                                if let (Some(point),Some((world,panel)))=(point,sides) {
                                    let point=crate::space::to_bevy(point);
                                    if point.is_finite() {next.panel=Some(panel);next.speaker=Some(world);target.0=Some(point);}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if *frame!=next {*frame=next;}
    if next.ui_camera.is_none() {
        for (entity,_) in &overlays {commands.entity(entity).despawn();}
        if let Some(entity)=pin.0.take().filter(|e|own_markers.contains(*e)) {
            commands.entity(entity).remove::<(IsDefaultUiCamera,OwnedDefault)>();
        }
    }
}

/// Retire last frame's publication before raw touch reads it in Scripts. Geometry
/// changes may be rebound by ordinary admission; a foreign target/viewport is not.
fn validate_existing(mut frame:ResMut<Frame>,mut lease:ResMut<ViewportLease>,
    windows:Query<(Entity,&Window),With<PrimaryWindow>>,
    cameras:Query<(&Camera,&Projection,&RenderTarget)>) {
    let Some(previous)=frame.classic else {return;};
    let camera=cameras.get(previous.camera).ok();
    let ownership_matches=camera.is_some_and(|(camera,_,target)|
        camera.viewport.as_ref().is_some_and(|v|same_viewport(v,&previous.viewport()))
            && target.normalize(Some(previous.window))==RenderTarget::default().normalize(Some(previous.window)));
    let valid=ownership_matches && camera.is_some_and(|(camera,projection,_)|
        camera.is_active && matches!(projection,Projection::Perspective(_)))
        && windows.single().is_ok_and(|(entity,w)|entity==previous.window && w.focused
            && Vec2::new(w.width(),w.height())==previous.window_size && w.scale_factor()==previous.density);
    if !valid {
        frame.classic=None;
        if !ownership_matches {lease.blocked=Some(previous.camera);}
    }
}

// Only restore what this lease actually wrote. An external owner wins, including
// an explicit reset to the full window; do not reacquire until Classic is left.
fn restore_viewport(lease:&mut ViewportLease,cameras:&mut Query<(Entity,&mut Camera,&Projection,&RenderTarget),Without<OverlayCamera>>) {
    if let Some(owned)=lease.owned.take() {
        if let Ok((_,mut camera,_,_))=cameras.get_mut(owned.entity) {
            if camera.viewport.as_ref().is_some_and(|v|same_viewport(v,&owned.written)) {camera.viewport=None;}
            else {lease.blocked=Some(owned.entity);}
        }
    }
}
type ClassicInputs<'w>=(ResMut<'w,Frame>,Res<'w,ClassicRequest>,
    Option<Res<'w,crate::dialogue_touch::classic_presenter::Surface>>);
type ClassicPublication<'w,'s>=(ResMut<'w,DefaultPin>,Query<'w,'s,Entity,With<OwnedDefault>>,OverlayCameras<'w,'s>,
    Query<'w,'s,&'static mut RenderTarget,With<OverlayCamera>>);
/// Commit the actual viewport and its public identity before either painter runs.
/// CameraUpdate still consumes this ordinary Camera change in PostUpdate. The
/// later Field Notes transform pass does not revoke a painter's selected frame.
fn apply_classic(mut commands:Commands,(mut frame,request,surface):ClassicInputs<'_>,
    windows:Query<(Entity,&Window),With<PrimaryWindow>>,mut lease:ResMut<ViewportLease>,
    mut cameras:Query<(Entity,&mut Camera,&Projection,&RenderTarget),Without<OverlayCamera>>,
    (mut pin,own_markers,mut overlays,mut overlay_targets):ClassicPublication<'_,'_>) {
    let prepared=surface.as_deref().and_then(|s|s.0.as_ref());
    let requested=request.0.filter(|r| {
        prepared.is_some_and(|p|*p==r.prepared)
            && windows.single().is_ok_and(|(e,w)|e==r.window && w.focused
                && Vec2::new(w.width(),w.height())==r.window_size && w.scale_factor()==r.density)
    });
    if lease.owned.as_ref().is_some_and(|l|requested.is_none_or(|r|l.entity!=r.camera)) {
        restore_viewport(&mut lease,&mut cameras);
    }
    let admitted=requested.filter(|request| {
        if let Ok((entity,mut camera,projection,target))=cameras.get_mut(request.camera) {
            let unchanged=lease.owned.as_ref().map_or(camera.viewport.is_none(),|l|
                l.entity==entity && camera.viewport.as_ref().is_some_and(|v|same_viewport(v,&l.written)));
            if unchanged && lease.blocked!=Some(entity) && camera.is_active && matches!(projection,Projection::Perspective(_))
                && target.normalize(Some(request.window))==RenderTarget::default().normalize(Some(request.window))
                && camera.computed.target_info.as_ref().is_some_and(|info|
                    info.physical_size==(request.window_size*request.density).round().as_uvec2()
                        && info.scale_factor==request.density) {
                let written=request.viewport();camera.viewport=Some(written.clone());
                lease.owned=Some(OwnedViewport {entity,written});
                return true;
            }
        }
        false
    });
    if let Some(viewport)=admitted {
        frame.classic=Some(viewport);
    } else {
        restore_viewport(&mut lease,&mut cameras);
        if let Some(request)=requested {lease.blocked=Some(request.camera);}
        if prepared.is_none() {lease.blocked=None;}
        frame.classic=None;
    }
    // Resolve's Commands have been applied at the ordered Resolve→Admit boundary.
    // Failure cleanup also completes before Ui, including only our own default pin.
    for (entity,mut camera) in &mut overlays {
        camera.is_active=frame.ui_camera==Some(entity);
        if camera.is_active {
            if let Ok((window,_))=windows.single() {
                if let Ok(mut target)=overlay_targets.get_mut(entity) {
                    *target=RenderTarget::Window(bevy::window::WindowRef::Entity(window));
                }
            }
        }
        if frame.ui_camera.is_none() {commands.entity(entity).despawn();}
    }
    if frame.ui_camera.is_none() {
        if let Some(entity)=pin.0.take().filter(|e|own_markers.contains(*e)) {
            commands.entity(entity).remove::<(IsDefaultUiCamera,OwnedDefault)>();
        }
    }
}

fn restore(owned: Owned, cameras: &mut Query<(Entity,&Camera,&Projection,&mut Transform),Without<OverlayCamera>>) {
    if let Ok((_,_,_,mut transform))=cameras.get_mut(owned.entity) {
        if transform.rotation==owned.written_rotation { transform.rotation=owned.base_rotation; }
    }
}

/// Aim the real camera so the speaker projects into the free half of the full view.
/// The world render target, viewport, projection and camera position remain owned by
/// their ordinary consumers; a foreign viewport suspends this framing.
fn framed_rotation(position: Vec3, point: Vec3, fov: f32, aspect: f32, screen_x: f32) -> Option<Quat> {
    let direction=point-position;
    if !direction.is_finite() || direction.length_squared()<0.0001
        || !fov.is_finite() || !(0.0..std::f32::consts::PI).contains(&fov)
        || !aspect.is_finite() || aspect<=0.0 || !screen_x.is_finite() { return None; }
    let center=Transform::from_translation(position).looking_at(point,Vec3::Y).rotation;
    let yaw=((2.0*screen_x-1.0)*(fov*0.5).tan()*aspect).atan();
    let rotation=center*Quat::from_rotation_y(yaw);
    rotation.is_finite().then_some(rotation)
}
type OverlayCameras<'w,'s> = Query<'w,'s,(Entity,&'static mut Camera),(With<OverlayCamera>,Without<PlayerCamera>)>;
fn apply(frame: Res<Frame>, target: Res<Target>, windows: Query<&Window,With<PrimaryWindow>>,
    players: Query<Entity,With<PlayerCamera>>,
    mut cameras: Query<(Entity,&Camera,&Projection,&mut Transform),Without<OverlayCamera>>,
    mut overlays: OverlayCameras<'_,'_>,mut lease: ResMut<Lease>) {
    for (entity,mut camera) in &mut overlays { camera.is_active=frame.ui_camera==Some(entity); }
    let active = frame.speaker.zip(target.0).zip(windows.single().ok());
    let Some(((rect,point),window))=active else {
        if let Some(owned)=lease.0.take() { restore(owned,&mut cameras); }
        return;
    };
    let Ok(entity)=players.single() else {
        if let Some(owned)=lease.0.take() {restore(owned,&mut cameras);}
        return;
    };
    if lease.0.as_ref().is_some_and(|l|l.entity!=entity) {
        if let Some(owned)=lease.0.take() {restore(owned,&mut cameras);}
    }
    let Ok((_,camera,projection,mut transform))=cameras.get_mut(entity) else {return};
    let Projection::Perspective(projection)=projection else {
        if let Some(owned)=lease.0.take() {restore(owned,&mut cameras);}
        return;
    };
    if camera.viewport.is_some() {
        if let Some(owned)=lease.0.take() {restore(owned,&mut cameras);}
        return;
    }
    let aspect=window.width()/window.height();
    let Some(rotation)=framed_rotation(transform.translation,point,projection.fov,aspect,rect.center().x/window.width()) else {
        if let Some(owned)=lease.0.take() {restore(owned,&mut cameras);}
        return;
    };
    let owned=lease.0.get_or_insert_with(||Owned {entity,base_rotation:transform.rotation,written_rotation:transform.rotation});
    if transform.rotation!=owned.written_rotation { owned.base_rotation=transform.rotation; }
    transform.rotation=rotation;
    owned.written_rotation=rotation;
}


#[cfg(test)]
mod overlay_order_tests {
    use super::*;
    fn default_camera(world:&mut World)->Option<Entity> {
        let mut state=bevy::ecs::system::SystemState::<DefaultUiCamera>::new(world);
        state.get(world).expect("valid default UI query").get()
    }
    #[test]
    fn retained_overlay_follows_new_active_camera_order_without_stealing_hud_routing_and_cleans_up() {
        crate::dialogue_touch::classic_presenter::with_barter_surface(|mut app,window| {
            let frame=*app.world().resource::<Frame>();
            let overlay=frame.ui_camera.unwrap();let player=frame.classic.unwrap().camera;
            let original_default=default_camera(app.world_mut()).unwrap();
            let original_order=app.world().get::<Camera>(overlay).unwrap().order;
            let original_target=app.world().get::<RenderTarget>(overlay).unwrap().normalize(Some(window));
            let hud=app.world_mut().spawn((Node {width:px(80.0),height:px(24.0),..default()},
                Pickable::IGNORE,Name::new("Ordinary default UI routing witness"))).id();
            let high_order=original_order.saturating_add(20);
            let competitor=app.world_mut().spawn((Camera2d,Camera {order:high_order,is_active:false,..default()},
                RenderTarget::Window(bevy::window::WindowRef::Entity(window)))).id();
            app.update();
            assert_eq!(app.world().get::<Camera>(overlay).unwrap().order,original_order,"inactive camera does not move overlay");
            assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(),Some(original_default));
            app.world_mut().get_mut::<Camera>(competitor).unwrap().is_active=true;
            app.update();
            assert_eq!(app.world().resource::<Frame>().ui_camera,Some(overlay),"retained camera identity");
            let actual=app.world().get::<Camera>(overlay).unwrap();
            assert!(actual.order>app.world().get::<Camera>(competitor).unwrap().order,"new active camera cannot cover retained dialogue");
            assert!(actual.is_active && actual.viewport.is_none());
            assert_eq!(app.world().get::<RenderTarget>(overlay).unwrap().normalize(Some(window)),original_target);
            match actual.output_mode {
                CameraOutputMode::Write {blend_state,clear_color}=>{
                    assert_eq!(blend_state,Some(BlendState::ALPHA_BLENDING));
                    assert!(matches!(clear_color,ClearColorConfig::None));
                },
                _=>panic!("order refresh must preserve alpha composition"),
            }
            assert_eq!(default_camera(app.world_mut()),Some(original_default));
            assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(),Some(original_default),"ordinary HUD routing remains pinned to its original camera");
            let other_window=app.world_mut().spawn(Window::default()).id();
            let foreign=app.world_mut().spawn((Camera2d,Camera {order:high_order.saturating_add(30),..default()},
                RenderTarget::Window(bevy::window::WindowRef::Entity(other_window)))).id();
            app.update();
            let raised=app.world().get::<Camera>(overlay).unwrap().order;
            assert!(raised>high_order);
            assert!(raised<app.world().get::<Camera>(foreign).unwrap().order,"other-window camera is outside the ordering scope");
            app.world_mut().get_mut::<Camera>(competitor).unwrap().is_active=false;app.update();
            assert_eq!(app.world().get::<Camera>(overlay).unwrap().order,original_order);
            app.world_mut().despawn(competitor);
            app.world_mut().resource_mut::<TouchDialogue>().enabled=false;app.update();
            assert!(app.world().get_entity(overlay).is_err());
            assert!(app.world().resource::<Frame>().ui_camera.is_none());
            assert!(app.world().resource::<DefaultPin>().0.is_none());
            assert!(app.world().get::<OwnedDefault>(original_default).is_none());
            assert!(app.world().get::<Camera>(player).unwrap().viewport.is_none());
            assert_eq!(default_camera(app.world_mut()),Some(original_default));
            assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(),Some(original_default));
            // A genuine foreign default-marker owner remains authoritative through
            // another open/close cycle; refreshing our order cannot remove it.
            app.world_mut().resource_mut::<TouchDialogue>().enabled=true;app.update();
            app.world_mut().entity_mut(foreign).insert(IsDefaultUiCamera);app.update();
            let reopened=app.world().resource::<Frame>().ui_camera.unwrap();
            assert!(app.world().get::<IsDefaultUiCamera>(foreign).is_some());
            assert_eq!(default_camera(app.world_mut()),Some(foreign));
            assert_eq!(app.world().get::<ComputedUiTargetCamera>(hud).unwrap().get(),Some(foreign));
            app.world_mut().resource_mut::<TouchDialogue>().enabled=false;app.update();
            assert!(app.world().get_entity(reopened).is_err());
            assert!(app.world().get::<IsDefaultUiCamera>(foreign).is_some());
            assert_eq!(app.world().get::<Camera>(foreign).unwrap().order,high_order.saturating_add(30));
            assert_eq!(default_camera(app.world_mut()),Some(foreign));
        });
    }

    #[test]
    fn wrist_terminal_split_uses_enlarged_side_panel_and_vertical_space() {
        let size = Vec2::new(880.0, 396.0);
        let safe = SafeInsets { left: 24.0, top: 0.0, right: 24.0, bottom: 16.0 };
        let safe_w = size.x - safe.left - safe.right;
        let safe_h = size.y - safe.top - safe.bottom;
        let (speaker, panel) = device_panel_rectangles(size, safe, Side::Left).unwrap();
        assert!(panel.width() / safe_w >= 0.60, "Wrist Terminal panel share {}", panel.width() / safe_w);
        assert!(panel.height() / safe_h >= 0.95, "Wrist Terminal height share {}", panel.height() / safe_h);
        assert!(speaker.max.x < panel.min.x);
    }
}
