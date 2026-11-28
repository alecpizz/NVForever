//! The water camera's cloud dome and the small linear image it composes.
//! Its trace comes from the mirrored eye. The scope keeps seeing the flat layers.
use std::sync::{Arc, Mutex};

use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use super::{Reflection, ReflectionCamera, ReflectionGate};
use crate::medium::{far_image, MediumOutput, MediumRequest};
use crate::volumetric_clouds::{compose_uniform, CloudVolumeMaterial, CloudVolumeProxy, CloudVolumeShown, FLAT_CLOUD_LAYER};

pub(super) const MIRROR_CLOUD_LAYER: usize = 23;

/// The render world reports the image it wrote after the proxy's own draw pipeline is ready.
/// Every replacement gets a fresh report slot too, so a late render cannot acknowledge it.
#[derive(Resource, Default)]
pub(crate) struct ReflectedClouds {
    pub image: Option<Handle<Image>>,
    pub ready: Arc<Mutex<Option<AssetId<Image>>>>,
    pub proxy: Option<Entity>,
    target: Option<Target>,
    material: Option<Handle<CloudVolumeMaterial>>,
}

#[derive(Clone, Copy, PartialEq)]
struct Target {
    plane: (u32, f32),
    size: UVec2,
    main_proxy: Entity,
    mesh: AssetId<Mesh>,
    sky_cuts: u32,
    epoch: u32,
}

/// One quarter of each mirror dimension, limited to 256 by 144 with the same aspect.
fn trace_size(mirror: UVec2) -> UVec2 {
    let extent = mirror.max(UVec2::ONE).as_vec2() * 0.25;
    let scale = (256.0 / extent.x).min(144.0 / extent.y).min(1.0);
    (extent * scale).round().as_uvec2().max(UVec2::ONE)
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ReflectedClouds>().add_systems(
        PostUpdate,
        follow
            .run_if(|state: Res<ReflectedClouds>, request: Option<Res<MediumRequest>>| {
                state.image.is_some() || request.is_some_and(|r| r.trace.is_some())
            })
            .after(super::update_reflection)
            .after(crate::volumetric_clouds::show_volume)
            .before(bevy::asset::AssetEventSystems)
            .before(VisibilitySystems::VisibilityPropagate),
    );
}

impl ReflectedClouds {
    fn release(&mut self, commands: &mut Commands, images: &mut Assets<Image>, materials: &mut Assets<CloudVolumeMaterial>) {
        if let Some(e) = self.proxy.take() {
            commands.entity(e).try_despawn();
        }
        if let Some(m) = self.material.take() {
            materials.remove(&m);
        }
        if let Some(image) = self.image.take() {
            images.remove(&image);
        }
        self.target = None;
        self.ready = Arc::default();
    }
}

#[allow(clippy::too_many_arguments)]
fn follow(
    mut commands: Commands,
    reflection: Option<Res<Reflection>>,
    gate: Res<ReflectionGate>,
    request: Res<MediumRequest>,
    medium: Res<MediumOutput>,
    shown: Res<CloudVolumeShown>,
    main: Query<(Entity, &Mesh3d, &GlobalTransform), With<CloudVolumeProxy>>,
    mut mirror: Query<(&Camera, &mut RenderLayers), With<ReflectionCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<CloudVolumeMaterial>>,
    mut state: ResMut<ReflectedClouds>,
) {
    let mut camera = mirror.single_mut().ok();
    let main_proxy = main.single().ok();
    let target = (|| {
        let (camera, _) = camera.as_ref()?;
        if !camera.is_active || request.trace.is_none() {
            return None;
        }
        let (main_proxy, mesh, _) = main_proxy?;
        let size = images.get(&reflection.as_ref()?.image)?.size();
        Some(Target { plane: gate.plane?, size: trace_size(size), main_proxy, mesh: mesh.id(), sky_cuts: request.sky_cuts, epoch: medium.epoch })
    })();
    if state.target != target {
        state.release(&mut commands, &mut images, &mut materials);
        if let (Some(t), Some((parent, mesh, transform))) = (target, main_proxy) {
            let mut buffer = far_image(t.size);
            buffer.texture_descriptor.label = Some("water_reflected_clouds");
            let image = images.add(buffer);
            let material = materials.add(CloudVolumeMaterial {
                params: compose_uniform(crate::lights::tone_curve(), nvf_assets::authored::physical_pbr(), true),
                far: Some(image.clone()),
            });
            // Share the main proxy's exact dome depth. Transform propagation has already
            // run this frame, so seed the new child's world transform from its parent.
            let proxy = commands.spawn((
                Name::new("Reflected volumetric clouds"), ChildOf(parent), mesh.clone(),
                MeshMaterial3d(material.clone()), Transform::IDENTITY, *transform,
                RenderLayers::layer(MIRROR_CLOUD_LAYER), NotShadowCaster, NotShadowReceiver,
            )).id();
            state.target = Some(t);
            state.image = Some(image);
            state.proxy = Some(proxy);
            state.material = Some(material);
        }
    }
    let drawing = shown.0 && state.image.as_ref().is_some_and(|image| {
        state.ready.lock().is_ok_and(|ready| *ready == Some(image.id()))
    });
    if let Some(material) = state.material.as_ref() {
        let params = compose_uniform(crate::lights::tone_curve(), nvf_assets::authored::physical_pbr(), !drawing);
        if materials.get(material).is_some_and(|m| m.params != params) {
            if let Some(mut m) = materials.get_mut(material) {
                m.params = params;
            }
        }
    }
    if let Some((_, layers)) = camera.as_mut() {
        let has_flats = layers.intersects(&RenderLayers::layer(FLAT_CLOUD_LAYER));
        if drawing && has_flats {
            **layers = layers.clone().without(FLAT_CLOUD_LAYER);
            info!("water reflection: volumetric clouds ready");
        } else if !drawing && !has_flats {
            **layers = layers.clone().with(FLAT_CLOUD_LAYER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_extent_follows_the_mirror_in_landscape_and_portrait() {
        for (mirror, expected) in [
            ([640, 360], [160, 90]), ([1920, 1080], [256, 144]), ([1920, 540], [256, 72]),
            ([540, 960], [81, 144]), ([0, 0], [1, 1]),
        ] {
            assert_eq!(trace_size(mirror.into()), UVec2::from(expected));
        }
    }

    #[test]
    fn the_mirror_keeps_flat_clouds_until_its_current_target_is_ready() {
        use crate::medium::SourceFrame;
        let mut app = App::new();
        app.init_resource::<Assets<Image>>().init_resource::<Assets<CloudVolumeMaterial>>()
            .init_resource::<ReflectedClouds>().init_resource::<MediumOutput>()
            .init_resource::<CloudVolumeShown>().insert_resource(ReflectionGate { plane: Some((7, 100.0)), last_in_view: None })
            .insert_resource(MediumRequest { trace: Some(SourceFrame {
                clouds: default(), composer: None, composer_entity: None, distance_scale: 1.0, noise: None, weather: None,
            }), ..default() })
            .add_systems(Update, follow);
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(far_image(UVec2::new(640, 360)));
        app.insert_resource(Reflection { image });
        app.world_mut().spawn((CloudVolumeProxy, Mesh3d::default(), GlobalTransform::IDENTITY));
        let layers = RenderLayers::from_layers(&[0, FLAT_CLOUD_LAYER, MIRROR_CLOUD_LAYER]);
        let mirror = app.world_mut().spawn((ReflectionCamera, Camera::default(), layers)).id();
        let flats_visible = |app: &App| app.world().get::<RenderLayers>(mirror).unwrap().intersects(&RenderLayers::layer(FLAT_CLOUD_LAYER));
        let report = |app: &App| {
            let s = app.world().resource::<ReflectedClouds>();
            (s.image.as_ref().unwrap().id(), s.ready.clone())
        };
        app.update();
        let (first, old_report) = report(&app);
        *old_report.lock().unwrap() = Some(first);
        app.update();
        assert!(flats_visible(&app), "the real volume is still warming");
        app.world_mut().resource_mut::<CloudVolumeShown>().0 = true;
        app.update();
        assert!(!flats_visible(&app));

        app.world_mut().resource_mut::<ReflectionGate>().plane = Some((7, 500.0));
        app.update();
        let (second, new_report) = report(&app);
        assert_ne!(first, second);
        *old_report.lock().unwrap() = Some(first);
        app.update();
        assert!(flats_visible(&app));
        *new_report.lock().unwrap() = Some(second);
        app.update();
        assert!(!flats_visible(&app));

        app.world_mut().get_mut::<Camera>(mirror).unwrap().is_active = false;
        app.update();
        let state = app.world().resource::<ReflectedClouds>();
        assert!(state.image.is_none() && state.proxy.is_none() && flats_visible(&app));
        assert!(app.world().resource::<Assets<CloudVolumeMaterial>>().is_empty());
        app.world_mut().get_mut::<Camera>(mirror).unwrap().is_active = true;
        app.update();
        assert_ne!(report(&app).0, second);
        assert!(flats_visible(&app));
    }
}
