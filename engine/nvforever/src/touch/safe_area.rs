//! Atomically read the Activity observer's physical safe area into common logical units.
use bevy::prelude::*;
#[cfg(target_os="android")]
use bevy::window::PrimaryWindow;
use nvf_play::touch_safe_area::SafeInsets;
pub(super) use nvf_play::touch_safe_area::Publish;
pub(super) fn install(app: &mut App) {
    app.init_resource::<SafeInsets>().configure_sets(PreUpdate,Publish.before(super::tray_editor::PublishMode));
    #[cfg(target_os="android")]
    app.add_systems(PreUpdate,poll.in_set(Publish));
}
#[cfg(target_os="android")]
fn poll(windows:Query<&Window,With<PrimaryWindow>>,time:Res<Time<Real>>,mut safe:ResMut<SafeInsets>,mut last:Local<Option<(UVec2,f32,f64)>>) {
    let Ok(window)=windows.single() else {*safe=SafeInsets::default();*last=None;return};
    let size=UVec2::new(window.physical_width(),window.physical_height());
    let density=window.scale_factor();
    let now=time.elapsed_secs_f64();
    if last.is_some_and(|(s,d,t)|s==size && d==density && now-t<0.2) {return}
    *last=Some((size,density,now));
    *safe=crate::android::activity_string("touchSafeInsets",None).ok()
        .and_then(|s|SafeInsets::from_snapshot(&s,size,density)).unwrap_or_default();
}
