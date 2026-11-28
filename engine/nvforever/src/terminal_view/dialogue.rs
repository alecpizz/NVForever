//! An independent dialogue owner of the actual terminal model renderer.
use super::{projection::GlassMap, spawn_monitor, RenderedMonitor, MONITOR_MODEL};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use bevy::window::PrimaryWindow;
use nvf_assets::model::ModelCtx;
use nvf_play::dialogue_device::{DeviceKind, DeviceView, Request};
use std::sync::Arc;

const DIALOGUE_MONITOR_LAYER: usize = 16;

pub(crate) struct DialogueTerminalPlugin;
impl Plugin for DialogueTerminalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialogueMonitor>()
            .add_systems(Update, manage.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync))
            .add_systems(PostUpdate, publish
                .after(bevy::transform::TransformSystems::Propagate)
                .after(bevy::camera::CameraUpdateSystems));
    }
}

#[derive(PartialEq)]
struct ProjectionKey {
    clip: Mat4,
    world: Mat4,
    physical: URect,
    logical: Rect,
}

struct Instance {
    request: Request,
    entities: Vec<Entity>,
    camera: Entity,
    image: Handle<Image>,
    glass: Arc<crate::pipboy_arm::Posed>,
    button: Option<Arc<crate::pipboy_arm::Posed>>,
    blockers: Vec<Arc<crate::pipboy_arm::Posed>>,
    published: Option<ProjectionKey>,
}

#[derive(Resource, Default)]
struct DialogueMonitor {
    active: Option<Instance>,
    failed: Option<Request>,
}

fn same_request(a: &Request, b: &Request) -> bool {
    a.stamp == b.stamp && a.image == b.image && a.camera == b.camera
        && a.target == b.target && a.canvas == b.canvas
}

fn remove_view(commands: &mut Commands, view: Option<&DeviceView>, request: &Request) {
    if view.is_some_and(|v| v.stamp.kind == DeviceKind::Terminal && v.stamp == request.stamp) {
        commands.remove_resource::<DeviceView>();
    }
}

#[allow(clippy::too_many_arguments)]
fn manage(
    mut commands: Commands,
    request: Option<Res<Request>>,
    view: Option<Res<DeviceView>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    owners: Query<()>,
    mut models: ModelCtx,
    mut plain: ResMut<Assets<StandardMaterial>>,
    mut state: ResMut<DialogueMonitor>,
) {
    let wanted = request.as_deref().filter(|r| {
        r.stamp.kind == DeviceKind::Terminal && r.valid() && owners.contains(r.stamp.owner)
            && windows.get(r.stamp.window).is_ok_and(|w| {
                w.scale_factor() == r.stamp.density
                    && r.stamp.display.min.cmpge(Vec2::ZERO).all()
                    && r.stamp.display.max.cmple(Vec2::new(w.physical_width() as f32,w.physical_height() as f32)).all()
            })
    });
    let keep = state.active.as_ref().zip(wanted).is_some_and(|(old,new)| same_request(&old.request,new));
    if !keep {
        if let Some(old) = state.active.take() {
            remove_view(&mut commands, view.as_deref(), &old.request);
            for entity in old.entities { commands.entity(entity).despawn(); }
        }
    }
    if state.failed.as_ref().zip(wanted).is_none_or(|(f, w)| !same_request(f, w)) {
        state.failed = None;
    }
    if state.active.is_some() || state.failed.is_some() { return; }
    let Some(request) = wanted else { return; };
    let Some(model) = models.model_with_cpu(MONITOR_MODEL) else { return; };
    // The image has integer texels; the mapping keeps the actual fractional
    // physical display rectangle used to present it.
    let size = request.stamp.display.size();
    let Some(RenderedMonitor {entities,camera,image,glass,button,blockers,..}) = spawn_monitor(
        &mut commands, &mut models, &mut plain, &model, &request.image, request.target,
        Extent3d {width:size.x.ceil() as u32,height:size.y.ceil() as u32,depth_or_array_layers:1},
        DIALOGUE_MONITOR_LAYER, "Dialogue terminal", true, -3, true, false, Vec3::ZERO,
    ) else {
        state.failed = Some(request.clone());
        return;
    };
    state.active = Some(Instance {request:request.clone(),entities,camera,image,
        glass:Arc::new(glass),button:button.map(Arc::new),blockers,published:None});
}

fn publish(
    mut commands: Commands,
    request: Option<Res<Request>>,
    view: Option<Res<DeviceView>>,
    cameras: Query<(&Camera,&GlobalTransform)>,
    mut state: ResMut<DialogueMonitor>,
) {
    let Some(instance) = state.active.as_mut() else { return; };
    if !request.as_deref().is_some_and(|r| same_request(&instance.request,r)) {
        remove_view(&mut commands,view.as_deref(),&instance.request);
        instance.published = None;
        return;
    }
    let ready = cameras.get(instance.camera).ok().and_then(|(camera,transform)| {
        Some((camera,transform,ProjectionKey {clip:camera.clip_from_view(),world:transform.to_matrix(),
            physical:camera.physical_viewport_rect()?,logical:camera.logical_viewport_rect()?}))
    });
    let Some((camera,transform,key)) = ready else {
        remove_view(&mut commands,view.as_deref(),&instance.request);
        instance.published = None;
        return;
    };
    if camera.is_active && instance.published.as_ref()==Some(&key)
        && view.as_deref().is_some_and(|v| v.matches(&instance.request) && v.image==instance.image) { return; }
    let Some((map,scale)) = GlassMap::new(instance.glass.clone(),instance.blockers.clone(),camera,transform,
        instance.request.stamp.display,instance.request.canvas,
        Affine2::from_scale(nvf_play::terminal::screen::TEXTURE_UNITS)) else {
        remove_view(&mut commands,view.as_deref(),&instance.request);
        instance.published = None;
        return;
    };
    let display = instance.request.stamp.display;
    let density = instance.request.stamp.density;
    let btn_size = (56.0 * density).max(48.0);
    let margin = (8.0 * density).max(6.0);
    let fallback_power = Rect::from_corners(
        Vec2::new((display.max.x - margin - btn_size).max(display.min.x), (display.max.y - margin - btn_size).max(display.min.y)),
        Vec2::new((display.max.x - margin).max(display.min.x + 1.0), (display.max.y - margin).max(display.min.y + 1.0)),
    );
    let map = map.with_power_button(instance.button.clone(), Some(fallback_power), density);
    commands.insert_resource(DeviceView {stamp:instance.request.stamp,image:instance.image.clone(),
        camera:instance.camera,scale,map:Arc::new(map)});
    instance.published = Some(key);
}

#[cfg(test)]
mod tests;
