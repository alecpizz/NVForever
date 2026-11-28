//! A dedicated view of the existing layout transaction, sharing the hotbar fitter.
use bevy::{prelude::*,window::{AppLifecycle,PrimaryWindow,WindowFocused}};
use nvf_play::touch_safe_area::SafeInsets;
pub(super) const KEY:&str="touch-tray-editor-v1";
#[derive(Resource,Default)]
pub(super) struct Requests(pub bool);
#[derive(Resource,Default)]
pub(super) struct PreviewMode(pub bool);
#[derive(Resource,Default)]
pub(super) struct Ready(pub bool);
#[derive(Clone,Debug)]
pub(super) struct Preview {pub bounds:Rect,pub slots:Vec<Rect>}
#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub(super) struct PublishMode;
#[cfg(test)]
pub(super) fn action_rect(index:usize,size:Vec2)->Rect {
    action_rect_with(index,size,SafeInsets::default())
}
pub(super) fn action_rect_with(index:usize,size:Vec2,safe:SafeInsets)->Rect {
    let usable=size-Vec2::new(safe.left+safe.right,safe.top+safe.bottom);
    let width=(usable.x-24.0).clamp(48.0,600.0);
    let height=(usable.y*0.2).clamp(48.0,56.0);
    let x=safe.left+(usable.x-width)*0.5+index as f32*width*0.25;
    Rect::from_corners(Vec2::new(x+2.0,size.y-safe.bottom-height-8.0),Vec2::new(x+width*0.25-2.0,size.y-safe.bottom-8.0))
}
pub(super) fn panel(size:Vec2,safe:SafeInsets)->Rect {let a=action_rect_with(0,size,safe);let b=action_rect_with(3,size,safe);Rect::from_corners(a.min-Vec2::splat(4.0),b.max+Vec2::splat(4.0))}
pub(super) fn install(app:&mut App) {
    app.init_resource::<Requests>().init_resource::<PreviewMode>().init_resource::<Ready>().init_resource::<SafeInsets>()
        .add_message::<AppLifecycle>().add_message::<WindowFocused>()
        .add_systems(PreUpdate,publish_mode.before(super::TouchInputSystems).in_set(PublishMode))
        .add_systems(PostStartup,(|mut ready:ResMut<Ready>|ready.0=true).after(nvf_play::settings::apply_stored));
    use nvf_play::settings::{self,Setting,ChoiceList};
    settings::add_choice_list(app,Setting {key:KEY,group:"Controls",label:"Edit hot tray",
        note:"Move and resize the fitted tray preview. Save keeps the current layout; Cancel restores it. Layout profile slots include the tray.",
        switches:"",get:|_|false,set:|_,_|{},slider:None},ChoiceList {
        names:|_|vec!["Ready".into(),"Edit tray".into()],chosen:|_|"Ready".into(),save:|_|"Ready".into(),
        load:|_,name|name=="Ready",ask_keep:false,pick:|w,name|{
            if name=="Ready" {return true}
            if name!="Edit tray" {return false}
            if w.get_resource::<nvf_play::bindings::Bindings>().is_some_and(|b|b.device==nvf_play::bindings::InputDevice::Controller) {return false}
            if w.get_resource::<super::Layout>().is_some_and(|l|!l.config.tray_supported) {return false}
            w.resource_mut::<Requests>().0=true;true
        },
    });
    settings::set_choice_action(app,KEY,|name|name=="Edit tray");
}
/// Final publication reads the current selector and persistent lifecycle state.
/// Ordinary pause/bar visibility is intentionally not part of preview admission.
pub(super) fn preview_allowed(state:Option<&super::TouchState>,bindings:Option<&nvf_play::bindings::Bindings>,switch:Option<&nvf_play::pad::TouchDeviceSwitch>)->bool {
    state.is_none_or(|s|!s.suspended)
        && bindings.is_none_or(|b|b.device==nvf_play::bindings::InputDevice::KeyboardMouse)
        && switch.is_none_or(nvf_play::pad::TouchDeviceSwitch::allows_actions)
}
fn publish_mode(mut layout:ResMut<super::Layout>,mut mode:ResMut<PreviewMode>,windows:Query<(Entity,&Window),With<PrimaryWindow>>,
    (safe,platform):(Res<SafeInsets>,Option<Res<nvf_play::pad::PlatformInputGate>>),
    mut focus:MessageReader<WindowFocused>,mut lifecycle:MessageReader<AppLifecycle>,
    mut previous:Local<Option<(Entity,Vec2,SafeInsets)>>) {
    let viewport=windows.single().ok().map(|(entity,w)|(entity,Vec2::new(w.width(),w.height()),*safe));
    let changed=*previous!=viewport;
    let interrupted=focus.read().any(|f|viewport.is_some_and(|v|v.0==f.window))
        | lifecycle.read().any(|l|matches!(l,AppLifecycle::Suspended|AppLifecycle::Running));
    *previous=viewport;
    layout.tray_safe = *safe;
    layout.tray_admitted=!changed && !interrupted
        && windows.single().is_ok_and(|(_,w)|w.focused && w.width()-safe.left-safe.right>=240.0 && w.height()-safe.top-safe.bottom>=160.0)
        && platform.as_deref().is_none_or(nvf_play::pad::PlatformInputGate::allows_actions);
    if changed || interrupted || !layout.tray_admitted {layout.cancel_editor_drag();layout.tray_preview=None;}
    mode.0=layout.tray_editing() && layout.tray_admitted;
}
